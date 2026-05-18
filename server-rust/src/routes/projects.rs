//! Project routes — mirrors server/modules/projects/projects.routes.ts
//! and file operations from server/index.js

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::auth::middleware::AuthUser;
use crate::db::repos::projects::ProjectsRepo;
use crate::shared::utils;

pub fn routes() -> Router {
    Router::new()
        .route("/", get(list_projects))
        .route("/{project_id}/files", get(get_files))
        .route("/{project_id}/file", get(read_file).put(write_file))
        .route("/{project_id}/files/create", post(create_file_or_dir))
        .route("/{project_id}/files/rename", put(rename_file))
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

// ── GET /api/projects — List all projects ────────────────────────────────────

async fn list_projects() -> Json<Value> {
    let projects = ProjectsRepo::list_projects();
    Json(json!({
        "success": true,
        "data": projects
    }))
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
