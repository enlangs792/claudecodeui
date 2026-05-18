mod db;
mod auth;
mod routes;
mod ws;
mod providers;
mod services;
mod shared;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    http::StatusCode,
    middleware, response::Json,
    routing::{get, post},
    Extension, Router,
};
use serde_json::{json, Value};
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing_subscriber::{EnvFilter, fmt};

use crate::auth::middleware::AuthUser;
use crate::db::migrations::initialize_database;
use crate::db::connection::init_pool;
use crate::auth::middleware as auth_mw;
use crate::providers::registry::ProviderRegistry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // Initialize database pool and run migrations
    init_pool();
    initialize_database();

    let app_root = find_app_root();
    tracing::info!("App root resolved to: {}", app_root.display());

    let cors = CorsLayer::permissive()
        .expose_headers([axum::http::header::HeaderName::from_static("x-refreshed-token")]);

    // Protected API middleware stack
    let mut protected = Router::new()
        .nest("/projects", routes::projects::routes())
        .nest("/git", routes::git::routes())
        .nest("/user", routes::user::routes())
        .nest("/settings", routes::settings::routes())
        .nest("/commands", routes::commands::routes())
        .nest("/agent", routes::agent::routes())
        .nest("/taskmaster", routes::taskmaster::routes())
        .nest("/mcp-utils", routes::mcp_utils::routes())
        .nest("/cursor", routes::cursor::routes())
        .nest("/gemini", routes::gemini::routes())
        .nest("/plugins", routes::plugins::routes())
        .nest("/providers", providers::routes::routes().with_state(Arc::new(ProviderRegistry::new())))
        .route("/system/update", post(system_update))
        .merge(routes::filesystem::routes());
    protected = protected.layer(middleware::from_fn(auth_mw::authenticate_token));

    let api_routes = Router::new()
        // Auth routes — public (no auth required)
        .nest("/auth", routes::auth::routes())
        // Protected routes
        .merge(protected)
        .layer(middleware::from_fn(auth_mw::validate_api_key));

    // Static file serving from dist/ (built frontend) and public/ (api-docs, favicon, etc.)
    let dist_path = app_root.join("dist");
    let public_path = app_root.join("public");

    let app = Router::new()
        .route("/health", get(health_check))
        .nest("/api", api_routes)
        .merge(ws::server::ws_router())
        .layer(cors);

    // Static file serving: serve files from dist/ first, then try public/,
    // then fall back to SPA handler (serves dist/index.html for non-API routes)
    let app = if dist_path.exists() {
        let dist_svc = ServeDir::new(&dist_path)
            .not_found_service(tower::service_fn(move |req: axum::http::Request<_>| {
                let public_path = public_path.clone();
                let dist_path = dist_path.clone();
                async move {
                    let uri_path = req.uri().path().trim_start_matches('/').to_string();
                    let path = Path::new(&uri_path);

                    // Try public/ directory for files missed in dist/
                    if !uri_path.is_empty() {
                        let public_file = public_path.join(path);
                        if public_file.is_file() {
                            let content_type = mime_guess::from_path(&public_file)
                                .first_or_octet_stream()
                                .to_string();
                            if let Ok(data) = tokio::fs::read(&public_file).await {
                                let resp = axum::http::Response::builder()
                                    .header("Content-Type", content_type)
                                    .body(axum::body::Body::from(data))
                                    .unwrap();
                                return Ok::<_, std::convert::Infallible>(resp);
                            }
                        }
                    }

                    // API routes that aren't matched should return 404, not the SPA
                    if uri_path.starts_with("api/") {
                        let resp = axum::http::Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .header("Content-Type", "application/json")
                            .body(axum::body::Body::from(r#"{"error":"Not found"}"#))
                            .unwrap();
                        return Ok(resp);
                    }

                    // SPA fallback: serve dist/index.html for non-file routes (no extension)
                    let has_extension = path.extension().is_some();
                    if !has_extension {
                        let index_path = dist_path.join("index.html");
                        if let Ok(content) = tokio::fs::read_to_string(&index_path).await {
                            let resp = axum::http::Response::builder()
                                .header("Content-Type", "text/html; charset=utf-8")
                                .header("Cache-Control", "no-cache, no-store, must-revalidate")
                                .body(axum::body::Body::from(content))
                                .unwrap();
                            return Ok(resp);
                        }
                    }

                    // Redirect to Vite dev server if dist/index.html doesn't exist (dev mode)
                    let redirect = format!("http://localhost:{}", crate::shared::config::VITE_PORT_DEFAULT);
                    let resp = axum::http::Response::builder()
                        .status(StatusCode::FOUND)
                        .header("Location", redirect)
                        .body(axum::body::Body::empty())
                        .unwrap();
                    Ok(resp)
                }
            }));
        app.fallback_service(dist_svc)
    } else {
        // Dev mode: no dist/ directory, redirect to Vite dev server
        let public_path_c = public_path.clone();
        app.fallback_service(tower::service_fn(move |_req: axum::http::Request<_>| {
            let public_path = public_path_c.clone();
            async move {
                // Try public/ files
                let uri_path = _req.uri().path().trim_start_matches('/').to_string();
                if !uri_path.is_empty() {
                    let public_file = public_path.join(&uri_path);
                    if public_file.is_file() {
                        let content_type = mime_guess::from_path(&public_file)
                            .first_or_octet_stream()
                            .to_string();
                        if let Ok(data) = tokio::fs::read(&public_file).await {
                            let resp = axum::http::Response::builder()
                                .header("Content-Type", content_type)
                                .body(axum::body::Body::from(data))
                                .unwrap();
                            return Ok::<_, std::convert::Infallible>(resp);
                        }
                    }
                }

                let redirect = format!("http://localhost:{}", crate::shared::config::VITE_PORT_DEFAULT);
                let resp = axum::http::Response::builder()
                    .status(StatusCode::FOUND)
                    .header("Location", redirect)
                    .body(axum::body::Body::empty())
                    .unwrap();
                Ok(resp)
            }
        }))
    };

    let port: u16 = std::env::var("SERVER_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());

    let addr = format!("{}:{}", host, port);
    tracing::info!("CloudCLI Rust Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "installMode": "git",
        "server": "rust"
    }))
}

/// POST /api/system/update — run system update (git pull or npm update)
/// Mirrors server/index.js lines 215-286.
async fn system_update(
    Extension(_user): Extension<AuthUser>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let app_root = find_app_root();
    let is_platform = crate::shared::config::IS_PLATFORM;
    let is_git = app_root.join(".git").exists();

    let (update_command, update_cwd) = if is_platform {
        ("npm run update:platform".to_string(), app_root.clone())
    } else if is_git {
        (
            "git checkout main && git pull && npm install".to_string(),
            app_root,
        )
    } else {
        (
            "npm install -g @cloudcli-ai/cloudcli@latest".to_string(),
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
        )
    };

    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&update_command)
        .current_dir(&update_cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to spawn update: {}", e)})),
            )
        })?;

    let output = child.wait_with_output().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(Json(json!({
            "success": true,
            "output": if stdout.is_empty() { "Update completed successfully".to_string() } else { stdout },
            "message": "Update completed. Please restart the server to apply changes."
        })))
    } else {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": "Update command failed",
                "output": stdout,
                "errorOutput": stderr
            })),
        ))
    }
}

/// Find the application root directory (parent of server-rust/).
/// Walks up from the binary location, CARGO_MANIFEST_DIR, or CWD to find
/// the project root (where dist/, public/, and server-rust/ reside).
fn find_app_root() -> PathBuf {
    // During development, CARGO_MANIFEST_DIR points to server-rust/
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let path = PathBuf::from(&manifest_dir);
        if path.join("Cargo.toml").exists() {
            return path.parent().map(|p| p.to_path_buf()).unwrap_or(path);
        }
    }

    // Walk up from the executable location
    let start = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut current = Some(start.as_path());
    while let Some(dir) = current {
        if dir.join("dist").exists() && dir.join("server-rust").exists() {
            return dir.to_path_buf();
        }
        if dir.join("Cargo.toml").exists() && dir.file_name().map(|n| n == "server-rust").unwrap_or(false) {
            // We're inside server-rust/, go up one
            if let Some(parent) = dir.parent() {
                return parent.to_path_buf();
            }
        }
        current = dir.parent();
    }

    // Fallback: CWD
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
