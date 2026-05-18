//! Plugin routes — mirrors server/routes/plugins.js
//!
//! GET    /                    — list installed plugins
//! GET    /{name}/manifest     — get single plugin manifest
//! GET    /{name}/assets/*path — serve plugin static files
//! PUT    /{name}/enable       — enable/disable plugin
//! POST   /install             — install plugin from git URL
//! POST   /{name}/update       — update plugin from git
//! ANY    /{name}/rpc/*path    — proxy requests to plugin subprocess (stub)
//! DELETE /{name}              — uninstall plugin

use axum::{
    extract::Path,
    http::StatusCode,
    response::{Json, Response},
    routing::{any, delete, get, post, put},
    Extension, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path as StdPath, PathBuf};

use crate::auth::middleware::AuthUser;

// ── Paths ─────────────────────────────────────────────────────────────────

/// Plugins directory: ~/.claude-code-ui/plugins
fn plugins_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Home directory not found")
        .join(".claude-code-ui")
        .join("plugins")
}

/// Plugins config file: ~/.claude-code-ui/plugins.json
fn plugins_config_path() -> PathBuf {
    dirs::home_dir()
        .expect("Home directory not found")
        .join(".claude-code-ui")
        .join("plugins.json")
}

/// Ensure plugins directory exists
async fn ensure_plugins_dir() -> PathBuf {
    let dir = plugins_dir();
    tokio::fs::create_dir_all(&dir).await.ok();
    dir
}

// ── Config I/O ─────────────────────────────────────────────────────────────

async fn read_config() -> Value {
    let path = plugins_config_path();
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or(json!({})),
        Err(_) => json!({}),
    }
}

async fn save_config(config: &Value) {
    let path = plugins_config_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    if let Ok(content) = serde_json::to_string_pretty(config) {
        let _ = tokio::fs::write(&path, &content).await;
    }
}

// ── Manifest Validation ────────────────────────────────────────────────────

const REQUIRED_MANIFEST_FIELDS: &[&str] = &["name", "displayName", "entry"];
const ALLOWED_TYPES: &[&str] = &["react", "module"];
const ALLOWED_SLOTS: &[&str] = &["tab"];

fn validate_manifest(manifest: &Value) -> Result<(), String> {
    if !manifest.is_object() {
        return Err("Manifest must be a JSON object".into());
    }
    for field in REQUIRED_MANIFEST_FIELDS {
        match manifest.get(*field) {
            Some(Value::String(s)) if !s.is_empty() => {}
            _ => return Err(format!("Missing or invalid required field: {}", field)),
        }
    }
    // Validate name format
    let name = manifest["name"].as_str().unwrap_or("");
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err(
            "Plugin name must only contain letters, numbers, hyphens, and underscores".into(),
        );
    }
    // Validate type
    if let Some(t) = manifest.get("type").and_then(|v| v.as_str()) {
        if !ALLOWED_TYPES.contains(&t) {
            return Err(format!(
                "Invalid plugin type: {}. Must be one of: {}",
                t,
                ALLOWED_TYPES.join(", ")
            ));
        }
    }
    // Validate slot
    if let Some(s) = manifest.get("slot").and_then(|v| v.as_str()) {
        if !ALLOWED_SLOTS.contains(&s) {
            return Err(format!("Invalid plugin slot: {}", s));
        }
    }
    // Validate entry is relative without ..
    if let Some(entry) = manifest.get("entry").and_then(|v| v.as_str()) {
        if entry.contains("..") || StdPath::new(entry).is_absolute() {
            return Err("Entry must be a relative path without \"..\"".into());
        }
    }
    // Validate server entry
    if let Some(server) = manifest.get("server") {
        if !server.is_null() {
            if let Some(s) = server.as_str() {
                if s.contains("..") || StdPath::new(s).is_absolute() {
                    return Err(
                        "Server entry must be a relative path string without \"..\"".into(),
                    );
                }
            } else {
                return Err("Server entry must be a string or null".into());
            }
        }
    }
    // Validate permissions
    if let Some(perms) = manifest.get("permissions") {
        if let Some(arr) = perms.as_array() {
            for p in arr {
                if !p.is_string() {
                    return Err("Permissions must be an array of strings".into());
                }
            }
        }
    }
    Ok(())
}

// ── Scanning ───────────────────────────────────────────────────────────────

/// Extract git remote URL from a plugin's .git/config
async fn git_remote_url(plugin_dir: &StdPath) -> Option<String> {
    let git_config_path = plugin_dir.join(".git").join("config");
    let content = tokio::fs::read_to_string(&git_config_path).await.ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(url_val) = trimmed.strip_prefix("url = ") {
            let mut url = url_val.trim().to_string();
            url = url.trim_end_matches(".git").to_string();
            // Convert SSH to HTTPS: git@github.com:user/repo -> https://github.com/user/repo
            if let Some(rest) = url.strip_prefix("git@") {
                if let Some(colon_pos) = rest.find(':') {
                    url = format!("https://{}/{}", &rest[..colon_pos], &rest[colon_pos + 1..]);
                }
            }
            // Strip embedded credentials
            if let Some(scheme_end) = url.find("://") {
                let after_proto = &url[scheme_end + 3..];
                if let Some(at_sign) = after_proto.find('@') {
                    url = format!(
                        "{}://{}",
                        &url[..scheme_end + 3],
                        &after_proto[at_sign + 1..]
                    );
                }
            }
            url = url.trim_end_matches('/').to_string();
            return Some(url);
        }
    }
    None
}

/// Scan the plugins directory and return all installed plugins
async fn scan_plugins() -> Vec<Value> {
    let dir = ensure_plugins_dir().await;
    let config = read_config().await;

    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut plugins: Vec<Value> = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    while let Ok(Some(entry)) = entries.next_entry().await {
        let ft = match entry.file_type().await {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !ft.is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().to_string();
        // Skip temp directories from in-progress installs
        if dir_name.starts_with(".tmp-") {
            continue;
        }

        let manifest_path = entry.path().join("manifest.json");
        let manifest_content = match tokio::fs::read_to_string(&manifest_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };
        let manifest: Value = match serde_json::from_str(&manifest_content) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("[Plugins] Failed to parse manifest for {}: {}", dir_name, e);
                continue;
            }
        };

        if let Err(e) = validate_manifest(&manifest) {
            tracing::warn!("[Plugins] Skipping {}: {}", dir_name, e);
            continue;
        }

        let name = manifest["name"].as_str().unwrap_or("").to_string();
        if seen_names.contains(&name) {
            tracing::warn!(
                "[Plugins] Skipping {}: duplicate plugin name \"{}\"",
                dir_name,
                name
            );
            continue;
        }
        seen_names.insert(name.clone());

        let repo_url = git_remote_url(&entry.path()).await;

        let enabled = config
            .get(&name)
            .and_then(|c| c.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        plugins.push(json!({
            "name": name,
            "displayName": manifest["displayName"].as_str().unwrap_or(""),
            "version": manifest.get("version").and_then(|v| v.as_str()).unwrap_or("0.0.0"),
            "description": manifest.get("description").and_then(|v| v.as_str()).unwrap_or(""),
            "author": manifest.get("author").and_then(|v| v.as_str()).unwrap_or(""),
            "icon": manifest.get("icon").and_then(|v| v.as_str()).unwrap_or("Puzzle"),
            "type": manifest.get("type").and_then(|v| v.as_str()).unwrap_or("module"),
            "slot": manifest.get("slot").and_then(|v| v.as_str()).unwrap_or("tab"),
            "entry": manifest["entry"].as_str().unwrap_or(""),
            "server": manifest.get("server").filter(|v| !v.is_null()),
            "permissions": manifest.get("permissions")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            "enabled": enabled,
            "dirName": dir_name,
            "repoUrl": repo_url,
            "serverRunning": false,
        }));
    }

    plugins
}

/// Find the directory path for a plugin by name
fn find_plugin_dir(name: &str) -> Option<PathBuf> {
    let dir = plugins_dir();
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        if !entry.file_type().ok().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        let content = std::fs::read_to_string(manifest_path).ok()?;
        let manifest: Value = serde_json::from_str(&content).ok()?;
        if manifest.get("name").and_then(|v| v.as_str()) == Some(name) {
            return Some(entry.path());
        }
    }
    None
}

/// Resolve an asset path within a plugin directory (prevents path traversal)
fn resolve_asset_path(plugin_dir: &StdPath, asset_path: &str) -> Option<PathBuf> {
    let resolved = plugin_dir.join(asset_path);
    let canonical = std::fs::canonicalize(&resolved).ok()?;
    let canonical_plugin = std::fs::canonicalize(plugin_dir).ok()?;
    if canonical.starts_with(&canonical_plugin) {
        Some(canonical)
    } else {
        None
    }
}

// ── Shell Helpers ──────────────────────────────────────────────────────────

/// Run a shell command and return (stdout, stderr)
async fn run_cmd(
    program: &str,
    args: &[&str],
    cwd: Option<&StdPath>,
) -> Result<(String, String), String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output().await.map_err(|e| format!("Failed to spawn {}: {}", program, e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_ = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        return Err(format!(
            "{} failed (exit {}): {}",
            program,
            code,
            stderr_.trim()
        ));
    }
    Ok((stdout, stderr_))
}

/// Run npm install (with --ignore-scripts) in the given directory
async fn npm_install(cwd: &StdPath) -> Result<(), String> {
    run_cmd("npm", &["install", "--ignore-scripts"], Some(cwd)).await?;
    // Run build if a build script exists
    let pkg_path = cwd.join("package.json");
    if let Ok(content) = tokio::fs::read_to_string(&pkg_path).await {
        if let Ok(pkg) = serde_json::from_str::<Value>(&content) {
            let has_build = pkg
                .get("scripts")
                .and_then(|s| s.get("build"))
                .and_then(|v| v.as_str())
                .is_some();
            if has_build {
                run_cmd("npm", &["run", "build"], Some(cwd)).await?;
            }
        }
    }
    Ok(())
}

// ── Route Setup ────────────────────────────────────────────────────────────

pub fn routes() -> Router {
    Router::new()
        .route("/", get(list_plugins))
        .route("/{name}/manifest", get(get_manifest))
        .route("/{name}/assets/*asset_path", get(serve_asset))
        .route("/{name}/enable", put(toggle_enable))
        .route("/install", post(install_plugin))
        .route("/{name}/update", post(update_plugin))
        .route("/{name}/rpc/*rpc_path", any(handle_rpc))
        .route("/{name}", delete(uninstall_plugin))
}

// ── Handlers ───────────────────────────────────────────────────────────────

/// GET / — list installed plugins
async fn list_plugins(Extension(_user): Extension<AuthUser>) -> Json<Value> {
    let plugins = scan_plugins().await;
    Json(json!({ "plugins": plugins }))
}

/// GET /{name}/manifest — get single plugin manifest
async fn get_manifest(
    Extension(_user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid plugin name"}))));
    }
    let plugins = scan_plugins().await;
    let plugin = plugins.into_iter().find(|p| p["name"] == name);
    match plugin {
        Some(p) => Ok(Json(p)),
        None => Err((StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found"})))),
    }
}

/// GET /{name}/assets/* — serve plugin static files
async fn serve_asset(
    Extension(_user): Extension<AuthUser>,
    Path((name, asset_path)): Path<(String, String)>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid plugin name"}))));
    }
    let asset_path = asset_path.trim_start_matches('/');
    if asset_path.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "No asset path specified"}))));
    }

    let plugin_dir = find_plugin_dir(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found"}))))?;

    let resolved = resolve_asset_path(&plugin_dir, asset_path)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Asset not found"}))))?;

    if !resolved.is_file() {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "Asset not found"}))));
    }

    let content_type = mime_guess::from_path(&resolved)
        .first_or_octet_stream()
        .to_string();

    let data = tokio::fs::read(&resolved)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to read asset"}))))?;

    let resp = Response::builder()
        .header("Content-Type", content_type)
        .header("Cache-Control", "no-store, no-cache, must-revalidate")
        .header("Pragma", "no-cache")
        .header("Expires", "0")
        .body(axum::body::Body::from(data))
        .unwrap();

    Ok(resp)
}

#[derive(Debug, Deserialize)]
struct EnableBody {
    enabled: bool,
}

/// PUT /{name}/enable — enable/disable plugin
async fn toggle_enable(
    Path(name): Path<String>,
    Json(body): Json<EnableBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Verify plugin exists
    let plugins = scan_plugins().await;
    if !plugins.iter().any(|p| p["name"] == name) {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found"}))));
    }

    let mut config = read_config().await;
    if let Some(obj) = config.as_object_mut() {
        let entry = obj.entry(name.clone()).or_insert(json!({}));
        if let Some(entry_obj) = entry.as_object_mut() {
            entry_obj.insert("enabled".into(), json!(body.enabled));
        }
    }
    save_config(&config).await;

    Ok(Json(json!({
        "success": true,
        "name": name,
        "enabled": body.enabled
    })))
}

#[derive(Debug, Deserialize)]
struct InstallBody {
    url: String,
}

/// POST /install — install plugin from git URL
async fn install_plugin(
    Json(body): Json<InstallBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let url = body.url.trim().to_string();
    if url.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "\"url\" is required and must be a string"}))));
    }
    if !url.starts_with("https://") && !url.starts_with("git@") {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "URL must start with https:// or git@"}))));
    }

    // Extract repo name from URL
    let url_clean = url.trim_end_matches(".git").trim_end_matches('/');
    let repo_name = url_clean.split('/').last().filter(|n| !n.is_empty())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "Could not determine a valid directory name from the URL"}))))?;

    if !repo_name.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_') {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid repo name extracted from URL"}))));
    }

    let plugins_dir = ensure_plugins_dir().await;
    let target_dir = plugins_dir.join(repo_name);

    // Ensure no name collision
    if target_dir.exists() {
        return Err((StatusCode::CONFLICT, Json(json!({"error": format!("Plugin directory \"{}\" already exists", repo_name)}))));
    }

    // Clone into temp directory, then move into place atomically
    let temp_dir = {
        let mut p = plugins_dir.join(format!(".tmp-{}-", repo_name));
        // Add random suffix
        p.push("");
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        p.set_file_name(format!(".tmp-{}-{}", repo_name, ts));
        p
    };
    tokio::fs::create_dir_all(&temp_dir).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to create temp dir: {}", e)})))
    })?;

    // Cleanup temp on error
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&temp_dir);
    };

    // git clone --depth 1
    let _ = run_cmd("git", &["clone", "--depth", "1", "--", &url, temp_dir.to_str().unwrap_or("")], None)
        .await
        .map_err(|e| {
            cleanup();
            (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Failed to install plugin: {}", e)})))
        })?;

    // Validate manifest exists
    let manifest_path = temp_dir.join("manifest.json");
    let manifest_content = tokio::fs::read_to_string(&manifest_path).await.map_err(|_| {
        cleanup();
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Cloned repository does not contain a manifest.json"})))
    })?;

    let manifest: Value = serde_json::from_str(&manifest_content).map_err(|_| {
        cleanup();
        (StatusCode::BAD_REQUEST, Json(json!({"error": "manifest.json is not valid JSON"})))
    })?;

    validate_manifest(&manifest).map_err(|e| {
        cleanup();
        (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid manifest: {}", e)})))
    })?;

    // Check for duplicate name
    let existing = scan_plugins().await;
    let manifest_name = manifest["name"].as_str().unwrap_or("");
    if existing.iter().any(|p| p["name"] == manifest_name) {
        cleanup();
        return Err((StatusCode::CONFLICT, Json(json!({"error": format!("A plugin named \"{}\" is already installed", manifest_name)}))));
    }

    // Run npm install if package.json exists
    let pkg_path = temp_dir.join("package.json");
    if pkg_path.exists() {
        if let Err(e) = npm_install(&temp_dir).await {
            cleanup();
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": format!("Failed to install dependencies: {}", e)}))));
        }
    }

    // Atomically move temp dir to target
    if let Err(e) = tokio::fs::rename(&temp_dir, &target_dir).await {
        cleanup();
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to move plugin into place: {}", e)}))));
    }

    // Enable by default
    let mut config = read_config().await;
    if let Some(obj) = config.as_object_mut() {
        let entry = obj.entry(manifest_name.to_string()).or_insert(json!({}));
        if let Some(entry_obj) = entry.as_object_mut() {
            entry_obj.insert("enabled".into(), json!(true));
        }
    }
    save_config(&config).await;

    Ok(Json(json!({
        "success": true,
        "plugin": manifest
    })))
}

/// POST /{name}/update — update plugin from git
async fn update_plugin(
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid plugin name"}))));
    }

    let plugin_dir = find_plugin_dir(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found"}))))?;

    // git pull --ff-only
    run_cmd("git", &["pull", "--ff-only", "--"], Some(&plugin_dir))
        .await
        .map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Failed to update plugin: {}", e)})))
        })?;

    // Re-validate manifest
    let manifest_path = plugin_dir.join("manifest.json");
    let manifest_content = tokio::fs::read_to_string(&manifest_path).await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "manifest.json not found after update"})))
    })?;

    let manifest: Value = serde_json::from_str(&manifest_content).map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "manifest.json is not valid JSON after update"})))
    })?;

    validate_manifest(&manifest).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid manifest after update: {}", e)})))
    })?;

    // Re-run npm install if package.json exists
    let pkg_path = plugin_dir.join("package.json");
    if pkg_path.exists() {
        npm_install(&plugin_dir).await.map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Failed to install dependencies: {}", e)})))
        })?;
    }

    Ok(Json(json!({
        "success": true,
        "plugin": manifest
    })))
}

/// ANY /{name}/rpc/* — proxy requests to plugin subprocess (stub)
async fn handle_rpc(
    Path((name, rpc_path)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid plugin name"}))));
    }
    // Stub — the Rust backend does not yet support plugin subprocess proxying
    Err((
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Plugin RPC not available in Rust backend",
            "details": format!("RPC path: {} / plugins/{}/rpc/{}", "method", name, rpc_path)
        })),
    ))
}

/// DELETE /{name} — uninstall plugin
async fn uninstall_plugin(
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid plugin name"}))));
    }

    let plugin_dir = find_plugin_dir(&name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found"}))))?;

    // Remove the directory
    tokio::fs::remove_dir_all(&plugin_dir)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to uninstall plugin: {}", e)})))
        })?;

    // Remove from config
    let mut config = read_config().await;
    if let Some(obj) = config.as_object_mut() {
        obj.remove(&name);
    }
    save_config(&config).await;

    Ok(Json(json!({
        "success": true,
        "name": name
    })))
}
