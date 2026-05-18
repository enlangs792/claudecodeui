//! Filesystem routes — browse directories and create folders
//!
//! GET  /api/browse-filesystem — list directories in a path
//! POST /api/create-folder — create a new directory

use axum::{
    extract::Query,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::shared::utils::{validate_workspace_path, workspaces_root};

pub fn routes() -> Router {
    Router::new()
        .route("/browse-filesystem", get(browse_filesystem))
        .route("/create-folder", post(create_folder))
}

#[derive(Debug, Deserialize)]
struct BrowseQuery {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateFolderBody {
    path: String,
}

/// GET /api/browse-filesystem — list directories in a path
async fn browse_filesystem(
    Query(query): Query<BrowseQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let root = workspaces_root();
    let target_path = query
        .path
        .map(|p| expand_tilde(&p, &root))
        .unwrap_or_else(|| root.to_string_lossy().to_string());

    // Resolve and normalize
    let target_path = PathBuf::from(&target_path);

    let target_str = target_path.to_string_lossy().to_string();

    // Validate path is within workspace root
    let validation = validate_workspace_path(&target_str).await;
    if !validation.valid {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": validation.error.unwrap_or_else(|| "Path not allowed".into())})),
        ));
    }

    let resolved_path = validation.resolved_path.unwrap_or(target_str);

    // Check it's a directory
    let meta = tokio::fs::metadata(&resolved_path)
        .await
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": format!("Directory not accessible: {}", e)})),
            )
        })?;
    if !meta.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Path is not a directory"})),
        ));
    }

    // Read directory entries
    let mut entries = tokio::fs::read_dir(&resolved_path)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

    let mut suggestions: Vec<Value> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path().to_string_lossy().into_owned();
            suggestions.push(json!({
                "name": name,
                "path": path,
                "type": "directory"
            }));
        }
    }

    // Sort: non-hidden first, then alphabetical (case-insensitive)
    suggestions.sort_by(|a, b| {
        let a_name = a["name"].as_str().unwrap_or("");
        let b_name = b["name"].as_str().unwrap_or("");
        let a_hidden = a_name.starts_with('.');
        let b_hidden = b_name.starts_with('.');
        if a_hidden != b_hidden {
            if a_hidden {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            }
        } else {
            a_name.to_lowercase().cmp(&b_name.to_lowercase())
        }
    });

    // Reorder: common directories first if browsing the workspace root
    let resolved_buf = PathBuf::from(&resolved_path);
    let resolved_canonical = std::fs::canonicalize(&resolved_buf).unwrap_or(resolved_buf);
    let root_canonical = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());

    let final_suggestions = if resolved_canonical == root_canonical {
        let common_dirs = [
            "Desktop", "Documents", "Projects", "Development", "Dev", "Code", "workspace",
        ];
        let mut common: Vec<Value> = Vec::new();
        let mut other: Vec<Value> = Vec::new();
        for s in suggestions {
            let name = s["name"].as_str().unwrap_or("");
            if common_dirs.contains(&name) {
                common.push(s);
            } else {
                other.push(s);
            }
        }
        common.extend(other);
        common
    } else {
        suggestions
    };

    Ok(Json(json!({
        "path": resolved_path,
        "suggestions": final_suggestions
    })))
}

/// POST /api/create-folder — create a new directory
async fn create_folder(
    Json(body): Json<CreateFolderBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let folder_path = body.path;
    if folder_path.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Path is required"})),
        ));
    }

    let root = workspaces_root();
    let expanded = expand_tilde(&folder_path, &root);

    let resolved_str = expanded.to_string();

    // Validate path is within workspace root
    let validation = validate_workspace_path(&resolved_str).await;
    if !validation.valid {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": validation.error.unwrap_or_else(|| "Path not allowed".into())})),
        ));
    }

    let target_path_str = validation.resolved_path.unwrap_or(resolved_str);
    let target_path = PathBuf::from(&target_path_str);

    // Check parent exists
    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Parent directory does not exist"})),
            ));
        }
    }

    // Check doesn't already exist
    if target_path.exists() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Folder already exists"})),
        ));
    }

    // Create directory (non-recursive, to match Node.js behavior)
    tokio::fs::create_dir(&target_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            (
                StatusCode::CONFLICT,
                Json(json!({"error": "Folder already exists"})),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        }
    })?;

    Ok(Json(json!({
        "success": true,
        "path": target_path.to_string_lossy()
    })))
}

/// Expand tilde (~) to workspace root path
fn expand_tilde(input: &str, root: &Path) -> String {
    let trimmed = input.trim();
    if trimmed == "~" {
        return root.to_string_lossy().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\")) {
        return root.join(rest).to_string_lossy().to_string();
    }
    trimmed.to_string()
}
