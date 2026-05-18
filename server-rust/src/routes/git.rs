//! Git routes — mirrors server/modules/git/git.routes.ts
//!
//! GET /api/git/status — returns current git branch and working tree status

use axum::{
    Extension,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;

pub fn routes() -> Router {
    Router::new()
        .route("/status", get(git_status))
}

async fn git_status(
    Extension(_user): Extension<AuthUser>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let branch = get_git_branch().unwrap_or_else(|_| "unknown".into());
    let dirty = is_git_dirty().unwrap_or(false);

    Ok(Json(json!({
        "branch": branch,
        "dirty": dirty
    })))
}

/// Run `git branch --show-current` to get the active branch name
fn get_git_branch() -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .map_err(|e| format!("Failed to execute git branch: {}", e))?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(branch)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Run `git status --porcelain` to check if working tree is dirty
fn is_git_dirty() -> Result<bool, String> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|e| format!("Failed to execute git status: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
