//! Taskmaster routes
//!
//! GET /taskmaster/status — check for .taskmaster directory in project dirs
//! GET /taskmaster/projects/:id/tasks — list tasks for a project (stub)

use axum::{
    extract::Path,
    Extension,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::db::repos::projects::ProjectsRepo;

pub fn routes() -> Router {
    Router::new()
        .route("/status", get(check_taskmaster_status))
        .route("/projects/{id}/tasks", get(list_tasks))
}

/// GET /taskmaster/status — scan project directories for .taskmaster
async fn check_taskmaster_status(
    Extension(_user): Extension<AuthUser>,
) -> Json<Value> {
    let projects = ProjectsRepo::list_projects();

    for project in &projects {
        let taskmaster_dir = std::path::Path::new(&project.project_path).join(".taskmaster");
        if taskmaster_dir.is_dir() {
            return Json(json!({
                "hasTaskmaster": true,
                "path": taskmaster_dir.to_string_lossy()
            }));
        }
    }

    Json(json!({
        "hasTaskmaster": false,
        "path": null
    }))
}

/// GET /taskmaster/projects/:id/tasks — stub task list
async fn list_tasks(
    Extension(_user): Extension<AuthUser>,
    Path(_id): Path<String>,
) -> Json<Value> {
    Json(json!({
        "tasks": []
    }))
}
