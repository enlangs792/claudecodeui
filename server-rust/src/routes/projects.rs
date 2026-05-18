//! Project routes — mirrors server/modules/projects/projects.routes.ts
//! and file operations from server/index.js

use axum::{
    extract::{Multipart, Path, Query},
    http::StatusCode,
    response::sse::{Event, Sse},
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;

use crate::db::repos::projects::ProjectsRepo;
use crate::db::repos::sessions::SessionsRepo;
use crate::shared::utils;

pub fn routes() -> Router {
    Router::new()
        .route("/", get(list_projects))
        .route("/archived", get(list_archived_projects))
        .route("/create-project", post(create_project_handler))
        .route("/migrate-legacy-stars", post(migrate_legacy_stars))
        .route("/clone-progress", get(clone_progress))
        .route("/{project_id}/sessions", get(get_project_sessions))
        .route("/{project_id}/sessions/{session_id}/token-usage", get(token_usage))
        .route("/{project_id}/taskmaster", get(get_taskmaster))
        .route("/{project_id}/rename", put(rename_project))
        .route("/{project_id}/toggle-star", post(toggle_star))
        .route("/{project_id}/restore", post(restore_project))
        .route("/{project_id}/upload-images", post(upload_images))
        .route("/{project_id}", delete(delete_project))
        .route("/{project_id}/files", get(get_files))
        .route("/{project_id}/file", get(read_file).put(write_file))
        .route("/{project_id}/files/create", post(create_file_or_dir))
        .route("/{project_id}/files/rename", put(rename_file))
        .route("/{project_id}/files/upload", post(upload_file))
        .route("/{project_id}/files", delete(delete_file_or_dir))
}

#[derive(Debug, Deserialize)]
struct FileQuery {
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileBody {
    #[serde(rename = "filePath")]
    file_path: Option<String>,
    content: Option<String>,
    name: Option<String>,
    #[serde(rename = "oldPath")]
    old_path: Option<String>,
    #[serde(rename = "newName")]
    new_name: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    path: Option<String>,
}

// ── GET /api/projects — List all projects with recent sessions ───────

async fn list_projects() -> Json<Value> {
    let projects = ProjectsRepo::list_projects();
    let items: Vec<Value> = projects
        .iter()
        .map(|p| {
            let display_name = p
                .custom_project_name
                .as_ref()
                .filter(|n| !n.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| {
                    std::path::Path::new(&p.project_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&p.project_path)
                        .to_string()
                });

            let (rows, has_more, total) =
                SessionsRepo::list_sessions_paginated(&p.project_path, 20, 0);
            let mut sessions = Vec::new();
            let mut cursor = Vec::new();
            let mut codex = Vec::new();
            let mut gemini = Vec::new();

            for row in &rows {
                let s = json!({
                    "id": row.session_id,
                    "summary": row.custom_name.clone().unwrap_or_default(),
                    "messageCount": 0,
                    "lastActivity": row.updated_at
                        .or(row.created_at)
                        .map(|t| t.and_utc().to_rfc3339())
                        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                });
                match row.provider.as_str() {
                    "cursor" => cursor.push(s),
                    "codex" => codex.push(s),
                    "gemini" => gemini.push(s),
                    _ => sessions.push(s),
                }
            }

            json!({
                "projectId": p.project_id,
                "path": p.project_path,
                "displayName": display_name,
                "fullPath": p.project_path,
                "isStarred": p.is_starred != 0,
                "sessions": sessions,
                "cursorSessions": cursor,
                "codexSessions": codex,
                "geminiSessions": gemini,
                "sessionMeta": {
                    "hasMore": has_more,
                    "total": total
                }
            })
        })
        .collect();

    Json(json!(items))
}

// ── GET /api/projects/:project_id/files — List file tree ─────────────────────

async fn get_files(
    Path(project_id): Path<String>,
    Query(_query): Query<FileQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = ProjectsRepo::get_project_path_by_id(&project_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Project not found"})),
            )
        })?;

    let files = get_file_tree(&project_path, 3, 0, true).await;

    Ok(Json(json!(files)))
}

// ── GET /api/projects/:project_id/file — Read a file ─────────────────────────

async fn read_file(
    Path(project_id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = ProjectsRepo::get_project_path_by_id(&project_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Project not found"})),
            )
        })?;

    let file_path = query.file_path.as_deref().or(query.path.as_deref()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "filePath is required"})),
        )
    })?;

    let resolved = resolve_within_project(&project_path, file_path).ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Path must be under project root"})),
        )
    })?;

    match tokio::fs::read_to_string(&resolved).await {
        Ok(content) => Ok(Json(json!({"content": content, "path": resolved}))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "File not found"})),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )),
    }
}

// ── PUT /api/projects/:project_id/file — Write a file ────────────────────────

async fn write_file(
    Path(project_id): Path<String>,
    Json(body): Json<FileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = ProjectsRepo::get_project_path_by_id(&project_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Project not found"})),
            )
        })?;

    let file_path = body.file_path.as_deref().or(body.path.as_deref()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "filePath is required"})),
        )
    })?;

    let content = body.content.as_deref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "content is required"})),
        )
    })?;

    let resolved = resolve_within_project(&project_path, file_path).ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Path must be under project root"})),
        )
    })?;

    tokio::fs::write(&resolved, content).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(json!({
        "success": true,
        "path": resolved,
        "message": "File saved successfully"
    })))
}

// ── POST /api/projects/:project_id/files/create — Create file/dir ────────────

async fn create_file_or_dir(
    Path(project_id): Path<String>,
    Json(body): Json<FileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = ProjectsRepo::get_project_path_by_id(&project_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Project not found"})),
            )
        })?;

    let name = body.name.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "name is required"})),
        )
    })?;

    let kind = body.kind.as_deref().unwrap_or("file");

    let parent = body.path.clone().unwrap_or_default();
    let target = if parent.is_empty() {
        PathBuf::from(&project_path).join(&name)
    } else {
        PathBuf::from(&project_path).join(&parent).join(&name)
    };

    let resolved = resolve_within_project(&project_path, &target.to_string_lossy())
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Path must be under project root"})),
            )
        })?;

    if resolved.exists() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Already exists"})),
        ));
    }

    if kind == "directory" {
        tokio::fs::create_dir_all(&resolved).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;
    } else {
        if let Some(parent_dir) = resolved.parent() {
            tokio::fs::create_dir_all(parent_dir).await.ok();
        }
        tokio::fs::write(&resolved, "").await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;
    }

    Ok(Json(json!({
        "success": true,
        "path": resolved.to_string_lossy(),
        "name": name,
        "type": kind
    })))
}

// ── PUT /api/projects/:project_id/files/rename — Rename file/dir ─────────────

async fn rename_file(
    Path(project_id): Path<String>,
    Json(body): Json<FileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = ProjectsRepo::get_project_path_by_id(&project_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Project not found"})),
            )
        })?;

    let old_path = body.old_path.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "oldPath is required"})),
        )
    })?;

    let new_name = body.new_name.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "newName is required"})),
        )
    })?;

    let resolved_old = resolve_within_project(&project_path, &old_path).ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Path must be under project root"})),
        )
    })?;

    if !resolved_old.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "File not found"})),
        ));
    }

    let new_path = resolved_old.parent().unwrap_or(&resolved_old).join(&new_name);
    let resolved_new = resolve_within_project(&project_path, &new_path.to_string_lossy())
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "New path must be under project root"})),
            )
        })?;

    tokio::fs::rename(&resolved_old, &resolved_new)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "oldPath": resolved_old.to_string_lossy(),
        "newPath": resolved_new.to_string_lossy(),
        "newName": new_name
    })))
}

// ── DELETE /api/projects/:project_id/files — Delete file/dir ─────────────────

async fn delete_file_or_dir(
    Path(project_id): Path<String>,
    Json(body): Json<FileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = ProjectsRepo::get_project_path_by_id(&project_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Project not found"})),
            )
        })?;

    let target = body.path.as_deref().or(body.file_path.as_deref()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "path is required"})),
        )
    })?;

    let resolved = resolve_within_project(&project_path, target).ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Path must be under project root"})),
        )
    })?;

    if !resolved.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "File not found"})),
        ));
    }

    if resolved.is_dir() {
        tokio::fs::remove_dir_all(&resolved).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;
    } else {
        tokio::fs::remove_file(&resolved).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;
    }

    Ok(Json(json!({
        "success": true,
        "path": resolved.to_string_lossy()
    })))
}

// ── Token Usage ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenUsageQuery {
    provider: Option<String>,
}

/// GET /api/projects/:project_id/sessions/:session_id/token-usage
/// Get LLM token usage for a specific session by provider.
async fn token_usage(
    Path((project_id, session_id)): Path<(String, String)>,
    Query(query): Query<TokenUsageQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let provider = query.provider.as_deref().unwrap_or("claude");
    let home_dir = dirs::home_dir().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Cannot determine home directory"})),
        )
    })?;

    // Sanitize session ID (only allow safe characters)
    let safe_session_id: String = session_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect();
    if safe_session_id.is_empty() || safe_session_id != session_id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid sessionId"})),
        ));
    }

    match provider {
        "cursor" | "gemini" => {
            return Ok(Json(json!({
                "used": 0,
                "total": 0,
                "breakdown": { "input": 0, "cacheCreation": 0, "cacheRead": 0 },
                "unsupported": true,
                "message": format!("Token usage tracking not available for {} sessions", provider)
            })));
        }
        "codex" => {
            handle_codex_token_usage(&home_dir, &safe_session_id).await
        }
        _ => {
            // Default: Claude
            handle_claude_token_usage(&project_id, &safe_session_id, &home_dir).await
        }
    }
}

/// Scan a directory recursively for a file containing the session ID
async fn find_session_file_recursive(
    dir: &std::path::Path,
    session_id: &str,
) -> Option<PathBuf> {
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false) {
            if let Some(found) = Box::pin(find_session_file_recursive(&path, session_id)).await {
                return Some(found);
            }
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.contains(session_id) && name.ends_with(".jsonl") {
                return Some(path);
            }
        }
    }
    None
}

/// Handle Claude token usage lookup
async fn handle_claude_token_usage(
    project_id: &str,
    session_id: &str,
    home_dir: &std::path::Path,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Resolve project path from DB
    let project_path = ProjectsRepo::get_project_path_by_id(project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Project not found"})),
        )
    })?;

    // Encode project path (replace non-alphanumeric chars with -)
    let encoded: String = project_path
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect();

    let project_dir = home_dir.join(".claude").join("projects").join(&encoded);
    let jsonl_path = project_dir.join(format!("{}.jsonl", session_id));

    // Path traversal check: ensure resolved path stays within project_dir
    let canonical_project = std::fs::canonicalize(&project_dir).unwrap_or(project_dir.clone());
    let canonical_jsonl = std::fs::canonicalize(&jsonl_path).unwrap_or(jsonl_path.clone());
    if !canonical_jsonl.starts_with(&canonical_project) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid path"})),
        ));
    }

    // Read the JSONL file
    let content = tokio::fs::read_to_string(&jsonl_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Session file not found"})),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        }
    })?;

    // Parse context window from env or use default
    let context_window = std::env::var("CONTEXT_WINDOW")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(160000);

    let mut input_tokens: i64 = 0;
    let mut cache_creation_tokens: i64 = 0;
    let mut cache_read_tokens: i64 = 0;

    // Scan from end for the latest assistant message with usage data
    let lines: Vec<&str> = content.lines().collect();
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<Value>(trimmed) {
            if entry["type"].as_str() == Some("assistant") {
                if let Some(usage) = entry["message"]["usage"].as_object() {
                    input_tokens = usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                    cache_creation_tokens = usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    cache_read_tokens = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);

                    // Check for top-level usage fields too (older format)
                    if input_tokens == 0 {
                        input_tokens = usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                    }
                    break;
                }
            }
        }
    }

    let total_used = input_tokens + cache_creation_tokens + cache_read_tokens;

    Ok(Json(json!({
        "used": total_used,
        "total": context_window,
        "breakdown": {
            "input": input_tokens,
            "cacheCreation": cache_creation_tokens,
            "cacheRead": cache_read_tokens
        }
    })))
}

/// Handle Codex token usage lookup
async fn handle_codex_token_usage(
    home_dir: &std::path::Path,
    session_id: &str,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let codex_sessions_dir = home_dir.join(".codex").join("sessions");

    let session_file = find_session_file_recursive(&codex_sessions_dir, session_id).await;

    let session_file = session_file.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Codex session file not found"})),
        )
    })?;

    let content = tokio::fs::read_to_string(&session_file).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Session file not found"})),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        }
    })?;

    let mut total_tokens: i64 = 0;
    let mut context_window: i64 = 200000;

    // Scan from end for the latest token_count event
    let lines: Vec<&str> = content.lines().collect();
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<Value>(trimmed) {
            if entry["type"].as_str() == Some("event_msg") {
                if let Some(payload) = entry["payload"].as_object() {
                    if payload.get("type").and_then(|v| v.as_str()) == Some("token_count") {
                        if let Some(info) = payload.get("info").and_then(|v| v.as_object()) {
                            if let Some(total_usage) = info.get("total_token_usage").and_then(|v| v.as_object()) {
                                total_tokens = total_usage
                                    .get("total_tokens")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);
                            }
                            if let Some(cw) = info.get("model_context_window").and_then(|v| v.as_i64()) {
                                context_window = cw;
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(Json(json!({
        "used": total_tokens,
        "total": context_window
    })))
}

// ── New Request Types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreateProjectBody {
    #[serde(rename = "path")]
    project_path: Option<String>,
    #[serde(rename = "customName")]
    custom_name: Option<String>,
    #[serde(rename = "workspaceType")]
    workspace_type: Option<Value>,
    #[serde(rename = "githubUrl")]
    github_url: Option<String>,
    #[serde(rename = "githubTokenId")]
    github_token_id: Option<Value>,
    #[serde(rename = "newGithubToken")]
    new_github_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MigrateStarsBody {
    #[serde(rename = "projectIds")]
    project_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SessionsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CloneProgressQuery {
    path: Option<String>,
    #[serde(rename = "githubUrl")]
    github_url: Option<String>,
    #[serde(rename = "githubTokenId")]
    github_token_id: Option<i64>,
    #[serde(rename = "newGithubToken")]
    new_github_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenameBody {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeleteQuery {
    force: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UploadImagesBody {
    images: Option<Vec<String>>,
}

// ── GET /api/projects/archived — List archived projects with sessions ─────────

async fn list_archived_projects() -> Json<Value> {
    let projects = ProjectsRepo::list_archived_projects();
    let items: Vec<Value> = projects
        .iter()
        .map(|p| {
            let display_name = p
                .custom_project_name
                .as_ref()
                .filter(|n| !n.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| {
                    std::path::Path::new(&p.project_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&p.project_path)
                        .to_string()
                });

            // Fetch ALL sessions (including archived) for archived project view
            let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>)> =
                crate::db::connection::with_db(|conn| {
                    use diesel::prelude::*;
                    use crate::db::schema::sessions;
                    let normalized = crate::shared::utils::normalize_project_path(&p.project_path);
                    sessions::table
                        .filter(sessions::project_path.eq(&normalized))
                        .order(sessions::updated_at.desc())
                        .select((
                            sessions::session_id,
                            sessions::provider,
                            sessions::custom_name,
                            sessions::created_at,
                            sessions::updated_at,
                        ))
                        .load::<(String, String, Option<String>, Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>)>(conn)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(sid, prov, cn, ca, ua)| {
                            (sid, prov, cn, ca.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()), ua.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()))
                        })
                        .collect::<Vec<_>>()
                        .into()
                });

            let total = rows.len();
            let mut sessions = Vec::new();
            let mut cursor = Vec::new();
            let mut codex = Vec::new();
            let mut gemini = Vec::new();

            for (session_id, provider, custom_name, created_at, updated_at) in &rows {
                let s = json!({
                    "id": session_id,
                    "summary": custom_name.clone().unwrap_or_default(),
                    "messageCount": 0,
                    "lastActivity": updated_at.clone()
                        .or_else(|| created_at.clone())
                        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                });
                match provider.as_str() {
                    "cursor" => cursor.push(s),
                    "codex" => codex.push(s),
                    "gemini" => gemini.push(s),
                    _ => sessions.push(s),
                }
            }

            json!({
                "projectId": p.project_id,
                "path": p.project_path,
                "displayName": display_name,
                "fullPath": p.project_path,
                "isStarred": p.is_starred != 0,
                "isArchived": true,
                "sessions": sessions,
                "cursorSessions": cursor,
                "codexSessions": codex,
                "geminiSessions": gemini,
                "sessionMeta": {
                    "hasMore": false,
                    "total": total
                }
            })
        })
        .collect();

    Json(json!({
        "success": true,
        "data": { "projects": items }
    }))
}

// ── GET /api/projects/:project_id/sessions — Paginated sessions ────────────────

async fn get_project_sessions(
    Path(project_id): Path<String>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project = ProjectsRepo::get_project_by_id(&project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Project not found", "code": "PROJECT_NOT_FOUND"})),
        )
    })?;

    let limit = query.limit.unwrap_or(20).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);

    let (rows, has_more, total) =
        SessionsRepo::list_sessions_paginated(&project.project_path, limit, offset);
    let mut sessions = Vec::new();
    let mut cursor = Vec::new();
    let mut codex = Vec::new();
    let mut gemini = Vec::new();

    for row in &rows {
        let s = json!({
            "id": row.session_id,
            "summary": row.custom_name.clone().unwrap_or_default(),
            "messageCount": 0,
            "lastActivity": row.updated_at
                .or(row.created_at)
                .map(|t| t.and_utc().to_rfc3339())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        });
        match row.provider.as_str() {
            "cursor" => cursor.push(s),
            "codex" => codex.push(s),
            "gemini" => gemini.push(s),
            _ => sessions.push(s),
        }
    }

    Ok(Json(json!({
        "projectId": project.project_id,
        "sessions": sessions,
        "cursorSessions": cursor,
        "codexSessions": codex,
        "geminiSessions": gemini,
        "sessionMeta": {
            "hasMore": has_more,
            "total": total
        }
    })))
}

// ── POST /api/projects/create-project — Create a new project ───────────────────

async fn create_project_handler(
    Json(body): Json<CreateProjectBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Reject legacy workspaceType field
    if body.workspace_type.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "workspaceType is no longer supported. Use the single create-project flow.",
                "code": "LEGACY_WORKSPACE_TYPE_UNSUPPORTED"
            })),
        ));
    }

    // Reject clone-related fields
    if body.github_url.is_some() || body.github_token_id.is_some() || body.new_github_token.is_some()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Repository cloning is not supported on create-project",
                "code": "CLONE_NOT_SUPPORTED_ON_CREATE_PROJECT",
                "details": "Use /api/projects/clone-progress for cloning workflows"
            })),
        ));
    }

    let project_path = body.project_path.as_deref().unwrap_or("");
    let normalized = utils::normalize_project_path(project_path);
    if normalized.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "path is required", "code": "PROJECT_PATH_REQUIRED"})),
        ));
    }

    // Validate workspace path
    let validation = utils::validate_workspace_path(&normalized).await;
    if !validation.valid {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid project path",
                "code": "INVALID_PROJECT_PATH",
                "details": validation.error
            })),
        ));
    }

    let resolved_path = validation.resolved_path.unwrap_or(normalized);

    // Ensure the directory exists
    tokio::fs::create_dir_all(&resolved_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    let result = ProjectsRepo::create_project_path(&resolved_path, body.custom_name.as_deref());

    if result.outcome == crate::shared::types::CreateProjectPathOutcome::ActiveConflict {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Project path already exists and is active",
                "code": "PROJECT_ALREADY_EXISTS",
                "details": format!("Project path already exists: {}", resolved_path)
            })),
        ));
    }

    let project_row = result
        .project
        .or_else(|| ProjectsRepo::get_project_path(&resolved_path))
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to resolve project after creation",
                    "code": "PROJECT_CREATE_FAILED"
                })),
            )
        })?;

    let display_name = project_row
        .custom_project_name
        .as_ref()
        .filter(|n| !n.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| {
            std::path::Path::new(&project_row.project_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&project_row.project_path)
                .to_string()
        });

    let outcome_str = match result.outcome {
        crate::shared::types::CreateProjectPathOutcome::Created => "created",
        crate::shared::types::CreateProjectPathOutcome::ReactivatedArchived => {
            "reactivated_archived"
        }
        crate::shared::types::CreateProjectPathOutcome::ActiveConflict => "active_conflict",
    };

    let message = if outcome_str == "reactivated_archived" {
        "Archived project path reused successfully"
    } else {
        "Project created successfully"
    };

    Ok(Json(json!({
        "success": true,
        "project": {
            "projectId": project_row.project_id,
            "path": project_row.project_path,
            "fullPath": project_row.project_path,
            "displayName": display_name,
            "customName": project_row.custom_project_name,
            "isArchived": project_row.is_archived != 0,
            "isStarred": project_row.is_starred != 0,
            "sessions": [],
            "cursorSessions": [],
            "codexSessions": [],
            "geminiSessions": [],
            "sessionMeta": {
                "hasMore": false,
                "total": 0
            }
        },
        "message": message
    })))
}

// ── POST /api/projects/migrate-legacy-stars — Migrate legacy stars ─────────────

async fn migrate_legacy_stars(Json(body): Json<MigrateStarsBody>) -> Json<Value> {
    let project_ids = body.project_ids.unwrap_or_default();

    let mut updated = 0usize;
    for pid in &project_ids {
        if let Some(project) = ProjectsRepo::get_project_by_id(pid) {
            if project.is_starred == 0 {
                ProjectsRepo::update_star_by_id(pid, true);
                updated += 1;
            }
        }
    }

    Json(json!({
        "success": true,
        "updated": updated
    }))
}

// ── GET /api/projects/clone-progress — SSE clone progress stub ─────────────────

async fn clone_progress(
    Query(_query): Query<CloneProgressQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::iter(vec![
        Ok(Event::default().data(r#"{"type":"progress","message":"Initializing clone..."}"#)),
        Ok(Event::default()
            .data(r#"{"type":"progress","message":"Repository cloning is not implemented in the Rust backend yet"}"#)),
        Ok(Event::default().data(r#"{"type":"error","message":"Clone not available in Rust backend"}"#)),
    ]);

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text(serde_json::json!({"type":"keepalive"}).to_string()),
    )
}

// ── GET /api/projects/:project_id/taskmaster — Detect .taskmaster dir ─────────

async fn get_taskmaster(
    Path(project_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let normalized = project_id.trim().to_string();
    if normalized.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "projectId is required",
                "code": "PROJECT_ID_REQUIRED"
            })),
        ));
    }

    let project_path = ProjectsRepo::get_project_path_by_id(&normalized).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Project not found",
                "code": "PROJECT_NOT_FOUND"
            })),
        )
    })?;

    let taskmaster_path = std::path::Path::new(&project_path).join(".taskmaster");
    let has_taskmaster = taskmaster_path.is_dir();

    let mut has_essential_files = false;
    let mut metadata = Value::Null;

    if has_taskmaster {
        let tasks_json = taskmaster_path.join("tasks").join("tasks.json");
        let _config_json = taskmaster_path.join("config.json");
        has_essential_files = tasks_json.exists();

        if has_essential_files {
            match tokio::fs::read_to_string(&tasks_json).await {
                Ok(content) => {
                    if let Ok(tasks_data) = serde_json::from_str::<Value>(&content) {
                        let tasks = tasks_data
                            .get("tasks")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_else(|| {
                                let mut all: Vec<Value> = Vec::new();
                                if let Some(obj) = tasks_data.as_object() {
                                    for val in obj.values() {
                                        if let Some(t) = val.get("tasks").and_then(|v| v.as_array())
                                        {
                                            all.extend_from_slice(t);
                                        }
                                    }
                                }
                                all
                            });

                        let total = tasks.len();
                        let mut done = 0u64;
                        let mut pending = 0u64;
                        let mut in_progress = 0u64;
                        let mut review = 0u64;

                        for task in &tasks {
                            match task
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("pending")
                            {
                                "done" => done += 1,
                                "in-progress" => in_progress += 1,
                                "review" => review += 1,
                                _ => pending += 1,
                            }
                        }

                        let completion_pct =
                            if total > 0 { (done as f64 / total as f64 * 100.0).round() as i64 } else { 0 };

                        if let Ok(meta) = tokio::fs::metadata(&tasks_json).await {
                            if let Ok(modified) = meta.modified() {
                                let dt: chrono::DateTime<chrono::Utc> =
                                    chrono::DateTime::from(modified);
                                metadata = json!({
                                    "taskCount": total,
                                    "subtaskCount": 0,
                                    "completed": done,
                                    "pending": pending,
                                    "inProgress": in_progress,
                                    "review": review,
                                    "completionPercentage": completion_pct,
                                    "lastModified": dt.to_rfc3339()
                                });
                            }
                        }
                    } else {
                        metadata = json!({"error": "Failed to parse tasks.json"});
                    }
                }
                Err(_) => {
                    metadata = json!({"error": "Failed to read tasks.json"});
                }
            }
        }
    }

    let status = if has_taskmaster && has_essential_files {
        "configured"
    } else {
        "not-configured"
    };

    Ok(Json(json!({
        "projectId": project_id,
        "projectPath": project_path,
        "taskmaster": {
            "hasTaskmaster": has_taskmaster,
            "hasEssentialFiles": has_essential_files,
            "metadata": metadata,
            "status": status
        }
    })))
}

// ── PUT /api/projects/:project_id/rename — Rename project ──────────────────────

async fn rename_project(
    Path(project_id): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project = ProjectsRepo::get_project_by_id(&project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Project not found"})),
        )
    })?;

    let name = body.display_name.as_deref().unwrap_or("");
    let trimmed = name.trim();

    if trimmed.is_empty() {
        let basename = std::path::Path::new(&project.project_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&project.project_path)
            .to_string();
        ProjectsRepo::update_custom_name_by_id(&project_id, &basename);
    } else {
        ProjectsRepo::update_custom_name_by_id(&project_id, trimmed);
    }

    Ok(Json(json!({"success": true})))
}

// ── POST /api/projects/:project_id/toggle-star — Toggle star ──────────────────

async fn toggle_star(
    Path(project_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let normalized = project_id.trim().to_string();
    if normalized.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "projectId is required",
                "code": "PROJECT_ID_REQUIRED"
            })),
        ));
    }

    let project = ProjectsRepo::get_project_by_id(&normalized).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Project not found",
                "code": "PROJECT_NOT_FOUND"
            })),
        )
    })?;

    let next_starred = project.is_starred == 0;
    ProjectsRepo::update_star_by_id(&normalized, next_starred);

    Ok(Json(json!({
        "success": true,
        "isStarred": next_starred
    })))
}

// ── POST /api/projects/:project_id/restore — Restore from archive ──────────────

async fn restore_project(
    Path(project_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project = ProjectsRepo::get_project_by_id(&project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Unknown projectId",
                "code": "PROJECT_NOT_FOUND"
            })),
        )
    })?;

    ProjectsRepo::update_archive_by_id(&project_id, false);

    Ok(Json(json!({
        "success": true,
        "data": {
            "projectId": project.project_id,
            "isArchived": false
        }
    })))
}

// ── DELETE /api/projects/:project_id — Delete or archive ──────────────────────

async fn delete_project(
    Path(project_id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project = ProjectsRepo::get_project_by_id(&project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Unknown projectId",
                "code": "PROJECT_NOT_FOUND"
            })),
        )
    })?;

    let force = query.force.as_deref() == Some("true");

    if !force {
        // Soft delete: set archived flag
        ProjectsRepo::update_archive_by_id(&project_id, true);
    } else {
        // Force delete: remove JSONL files, session rows, and project row
        let sessions = SessionsRepo::list_sessions(Some(&project.project_path));
        for session in &sessions {
            if let Some(ref jsonl_path) = session.jsonl_path {
                let path = std::path::Path::new(jsonl_path);
                if path.exists() {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
        }

        // Delete all session rows for this project path
        let normalized = utils::normalize_project_path(&project.project_path);
        crate::db::connection::with_db(|conn| {
            use diesel::prelude::*;
            use crate::db::schema::sessions;
            diesel::delete(
                sessions::table.filter(sessions::project_path.eq(&normalized)),
            )
            .execute(conn)
            .ok();
        });

        // Delete project row
        ProjectsRepo::delete_by_id(&project_id);
    }

    Ok(Json(json!({"success": true})))
}

// ── POST /api/projects/:project_id/upload-images — Upload base64 images ───────

async fn upload_images(
    Path(project_id): Path<String>,
    Json(body): Json<UploadImagesBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = ProjectsRepo::get_project_path_by_id(&project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Project not found"})),
        )
    })?;

    let images = body.images.unwrap_or_default();
    let upload_dir = std::path::Path::new(&project_path).join(".uploads");
    tokio::fs::create_dir_all(&upload_dir).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    use base64::Engine;
    let mut uploaded = Vec::new();
    for (i, img) in images.iter().enumerate() {
        let data = if let Some(stripped) = img.strip_prefix("data:image/") {
            // data:image/png;base64,<actual_data>
            if let Some(comma_pos) = stripped.find(',') {
                &stripped[comma_pos + 1..]
            } else {
                img
            }
        } else {
            img
        };

        let decoded = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Invalid base64 image at index {}", i)})),
            )
        })?;

        let ext = image_ext_from_bytes(&decoded);
        let filename = format!("{}_{}.{}", chrono::Utc::now().timestamp_micros(), i, ext);
        let filepath = upload_dir.join(&filename);

        tokio::fs::write(&filepath, &decoded).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

        uploaded.push(json!({
            "url": format!("/uploads/{}", filename),
            "path": filepath.to_string_lossy(),
            "filename": filename
        }));
    }

    Ok(Json(json!({
        "success": true,
        "images": uploaded
    })))
}

/// POST /{project_id}/files/upload — multipart file upload
async fn upload_file(
    Path(project_id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = ProjectsRepo::get_project_path_by_id(&project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Project not found"})),
        )
    })?;

    let mut uploaded = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        let file_name = field.file_name().unwrap_or("unnamed").to_string();
        let content = field.bytes().await.map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Failed to read file data"})),
            )
        })?;

        // Strip path separators from filename for safety
        let safe_name = file_name.replace('/', "_").replace('\\', "_");
        let upload_dir = std::path::Path::new(&project_path).join(".uploads");
        tokio::fs::create_dir_all(&upload_dir).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

        let file_path = upload_dir.join(&safe_name);
        tokio::fs::write(&file_path, &content).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

        uploaded.push(json!({
            "filename": safe_name,
            "path": file_path.to_string_lossy(),
            "size": content.len(),
        }));
    }

    Ok(Json(json!({
        "success": true,
        "files": uploaded
    })))
}

/// Determine image file extension from magic bytes
fn image_ext_from_bytes(bytes: &[u8]) -> &'static str {
    if bytes.len() < 4 {
        return "png";
    }
    if bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        "jpg"
    } else if bytes[0] == 0x89 && bytes[1] == b'P' && bytes[2] == b'N' && bytes[3] == b'G' {
        "png"
    } else if bytes[0] == b'G' && bytes[1] == b'I' && bytes[2] == b'F' {
        "gif"
    } else if bytes[0] == 0x52 && bytes[1] == 0x49 && bytes[2] == 0x46 && bytes[3] == 0x46 {
        "webp"
    } else {
        "png"
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve a path relative to project root and validate it stays within.
/// Uses canonicalize() for symlink resolution — not just string prefix matching.
fn resolve_within_project(project_root: &str, target: &str) -> Option<PathBuf> {
    let root = std::fs::canonicalize(project_root)
        .or_else(|_| std::path::absolute(project_root))
        .unwrap_or_else(|_| PathBuf::from(project_root));

    let resolved = if std::path::Path::new(target).is_absolute() {
        std::fs::canonicalize(target)
            .or_else(|_| std::path::absolute(target))
            .unwrap_or_else(|_| PathBuf::from(target))
    } else {
        let joined = root.join(target);
        std::fs::canonicalize(&joined)
            .or_else(|_| std::path::absolute(&joined))
            .unwrap_or(joined)
    };

    let canonical = std::fs::canonicalize(&resolved).unwrap_or(resolved);

    if canonical.starts_with(&root) {
        Some(canonical)
    } else {
        None
    }
}

/// Build a file tree (mirrors getFileTree from server/index.js)
async fn get_file_tree(dir: &str, max_depth: usize, depth: usize, _show_hidden: bool) -> Vec<Value> {
    let mut items = Vec::new();

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return items,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();

        // Skip heavy directories
        if matches!(name.as_str(), "node_modules" | "dist" | "build" | ".git" | ".svn" | ".hg") {
            continue;
        }

        let path = entry.path();
        let path_str = path.to_string_lossy().into_owned();
        let is_dir = entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false);

        let mut item = json!({
            "name": name,
            "path": path_str,
            "type": if is_dir { "directory" } else { "file" }
        });

        // Get file stats
        if let Ok(meta) = tokio::fs::metadata(&path).await {
            let permissions = if cfg!(unix) {
                use std::os::unix::fs::PermissionsExt;
                format!("{:o}", meta.permissions().mode() & 0o777)
            } else {
                "644".to_string()
            };
            item["size"] = json!(meta.len());
            item["modified"] = json!(meta.modified().map(|t| {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            }).unwrap_or_default());
            item["permissions"] = json!(permissions);
        }

        if is_dir && depth < max_depth {
            let children = Box::pin(get_file_tree(&path_str, max_depth, depth + 1, false)).await;
            item["children"] = json!(children);
        }

        items.push(item);
    }

    items.sort_by(|a, b| {
        let a_type = a["type"].as_str().unwrap_or("");
        let b_type = b["type"].as_str().unwrap_or("");
        if a_type != b_type {
            if a_type == "directory" { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }
        } else {
            a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
        }
    });

    items
}
