//! Taskmaster routes
//!
//! All routes are under /api/taskmaster/ prefix.
//! Mirrors server/routes/taskmaster.js
//!
//! Endpoints:
//! - GET    /installation-status        — check if task-master CLI is installed
//! - GET    /tasks/{project_id}          — load tasks from .taskmaster/tasks/tasks.json
//! - GET    /prd/{project_id}            — list PRD files in .taskmaster/docs/
//! - POST   /prd/{project_id}            — create/update PRD file
//! - GET    /prd/{project_id}/{file_name} — get specific PRD content
//! - POST   /parse-prd/{project_id}      — parse PRD to generate tasks (spawns npx)
//! - GET    /prd-templates               — return built-in PRD templates
//! - POST   /apply-template/{project_id}  — apply a PRD template
//! - POST   /init/{project_id}           — initialize TaskMaster in project
//! - POST   /add-task/{project_id}       — add new task
//! - PUT    /update-task/{project_id}/{task_id} — update task status/details

use axum::{
    extract::Path,
    http::StatusCode,
    response::Json,
    routing::{get, post, put},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

use crate::db::repos::projects::ProjectsRepo;

// ── Router ───────────────────────────────────────────────────────────────────

pub fn routes() -> Router {
    Router::new()
        .route("/installation-status", get(check_installation_status))
        .route("/tasks/{project_id}", get(get_tasks))
        .route("/prd/{project_id}", get(list_prd_files).post(create_prd_file))
        .route("/prd/{project_id}/{file_name}", get(get_prd_file))
        .route("/parse-prd/{project_id}", post(parse_prd))
        .route("/prd-templates", get(get_prd_templates))
        .route("/apply-template/{project_id}", post(apply_template))
        .route("/init/{project_id}", post(init_taskmaster))
        .route("/add-task/{project_id}", post(add_task))
        .route("/update-task/{project_id}/{task_id}", put(update_task))
}

// ── Request Body Types ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreatePrdBody {
    #[serde(rename = "fileName")]
    file_name: String,
    content: String,
}

#[derive(Deserialize)]
struct ParsePrdBody {
    #[serde(rename = "fileName")]
    file_name: Option<String>,
    #[serde(rename = "numTasks")]
    num_tasks: Option<u32>,
    append: Option<bool>,
}

#[derive(Deserialize)]
struct ApplyTemplateBody {
    #[serde(rename = "templateId")]
    template_id: String,
    #[serde(rename = "fileName")]
    file_name: Option<String>,
    customizations: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct AddTaskBody {
    prompt: Option<String>,
    title: Option<String>,
    description: Option<String>,
    priority: Option<String>,
    dependencies: Option<String>,
}

#[derive(Deserialize)]
struct UpdateTaskBody {
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    details: Option<String>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve project path from ID, returning 404 if not found.
fn resolve_project(project_id: &str) -> Result<String, (StatusCode, Json<Value>)> {
    ProjectsRepo::get_project_path_by_id(project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Project not found",
                "message": format!("Project \"{}\" does not exist", project_id)
            })),
        )
    })
}

/// Current timestamp in RFC 3339 format.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Today's date in YYYY-MM-DD format.
fn today_date() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// Spawn npx with the given args in the given working directory, pipe an
/// optional byte slice to stdin, and return (stdout, stderr, exit_code).
async fn run_npx(
    args: &[&str],
    cwd: &str,
    stdin_input: Option<&[u8]>,
) -> Result<(String, String, Option<i32>), String> {
    let mut cmd = tokio::process::Command::new("npx");
    cmd.args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn npx: {}", e))?;

    if let Some(input) = stdin_input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input).await;
            let _ = stdin.shutdown().await;
        }
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Failed to wait for npx: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code();

    Ok((stdout, stderr, code))
}

/// Extract tasks and current tag from parsed tasks.json data.
/// Handles legacy array format, simple { tasks: [...] } format,
/// and tagged { master: { tasks: [...] } } format.
fn extract_tasks(data: &Value) -> (Vec<Value>, String) {
    // Legacy format — top-level array
    if let Some(arr) = data.as_array() {
        return (arr.clone(), "master".to_string());
    }

    // Simple format — { tasks: [...] }
    if let Some(tasks) = data.get("tasks").and_then(|v| v.as_array()) {
        return (tasks.clone(), "master".to_string());
    }

    // Tagged format — { master: { tasks: [...] }, ... }
    if let Some(obj) = data.as_object() {
        // Try "master" tag first
        if let Some(master) = obj.get("master") {
            if let Some(tasks) = master.get("tasks").and_then(|v| v.as_array()) {
                return (tasks.clone(), "master".to_string());
            }
        }
        // Fall back to first available tag with tasks
        for (key, val) in obj {
            if let Some(tasks) = val.get("tasks").and_then(|v| v.as_array()) {
                return (tasks.clone(), key.clone());
            }
        }
    }

    (vec![], "master".to_string())
}

// ── 1. GET /installation-status ──────────────────────────────────────────────

/// Check if TaskMaster CLI is installed on the system.
async fn check_installation_status() -> Json<Value> {
    let which_output = std::process::Command::new("which")
        .arg("task-master")
        .output();

    match which_output {
        Ok(output) if output.status.success() => {
            let install_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

            let version = std::process::Command::new("task-master")
                .arg("--version")
                .output()
                .ok()
                .and_then(|v| {
                    if v.status.success() {
                        Some(String::from_utf8_lossy(&v.stdout).trim().to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "unknown".to_string());

            Json(json!({
                "success": true,
                "installation": {
                    "isInstalled": true,
                    "installPath": install_path,
                    "version": version,
                    "reason": null
                },
                "mcpServer": {
                    "hasMCPServer": false,
                    "reason": "MCP server detection not available in Rust backend"
                },
                "isReady": false
            }))
        }
        _ => Json(json!({
            "success": true,
            "installation": {
                "isInstalled": false,
                "installPath": null,
                "version": null,
                "reason": "TaskMaster CLI not found in PATH"
            },
            "mcpServer": {
                "hasMCPServer": false,
                "reason": "MCP server detection not available in Rust backend"
            },
            "isReady": false
        })),
    }
}

// ── 2. GET /tasks/{project_id} ──────────────────────────────────────────────

/// Load tasks from .taskmaster/tasks/tasks.json.
async fn get_tasks(
    Path(project_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    let tasks_path = PathBuf::from(&project_path)
        .join(".taskmaster")
        .join("tasks")
        .join("tasks.json");

    if !tasks_path.exists() {
        return Ok(Json(json!({
            "projectId": project_id,
            "tasks": [],
            "message": "No tasks.json file found"
        })));
    }

    let content = tokio::fs::read_to_string(&tasks_path)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to read tasks file",
                    "message": e.to_string()
                })),
            )
        })?;

    let tasks_data: Value = serde_json::from_str(&content).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to parse tasks file",
                "message": e.to_string()
            })),
        )
    })?;

    let (tasks, current_tag) = extract_tasks(&tasks_data);
    let now = now_iso();

    let transformed: Vec<Value> = tasks
        .iter()
        .map(|t| {
            let id = t.get("id").cloned().unwrap_or(Value::Null);
            let title = t
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled Task")
                .to_string();
            let description = t
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = t
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .to_string();
            let priority = t
                .get("priority")
                .and_then(|v| v.as_str())
                .unwrap_or("medium")
                .to_string();
            let dependencies = t
                .get("dependencies")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let created_at = t
                .get("createdAt")
                .or_else(|| t.get("created"))
                .and_then(|v| v.as_str())
                .unwrap_or(&now)
                .to_string();
            let updated_at = t
                .get("updatedAt")
                .or_else(|| t.get("updated"))
                .and_then(|v| v.as_str())
                .unwrap_or(&now)
                .to_string();
            let details = t
                .get("details")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let test_strategy = t
                .get("testStrategy")
                .or_else(|| t.get("test_strategy"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let subtasks = t
                .get("subtasks")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            json!({
                "id": id,
                "title": title,
                "description": description,
                "status": status,
                "priority": priority,
                "dependencies": dependencies,
                "createdAt": created_at,
                "updatedAt": updated_at,
                "details": details,
                "testStrategy": test_strategy,
                "subtasks": subtasks
            })
        })
        .collect();

    let pending = transformed.iter().filter(|t| t["status"] == "pending").count();
    let in_progress = transformed
        .iter()
        .filter(|t| t["status"] == "in-progress")
        .count();
    let done = transformed.iter().filter(|t| t["status"] == "done").count();
    let review = transformed.iter().filter(|t| t["status"] == "review").count();
    let deferred = transformed
        .iter()
        .filter(|t| t["status"] == "deferred")
        .count();
    let cancelled = transformed
        .iter()
        .filter(|t| t["status"] == "cancelled")
        .count();

    Ok(Json(json!({
        "projectId": project_id,
        "projectPath": project_path,
        "tasks": transformed,
        "currentTag": current_tag,
        "totalTasks": transformed.len(),
        "tasksByStatus": {
            "pending": pending,
            "in-progress": in_progress,
            "done": done,
            "review": review,
            "deferred": deferred,
            "cancelled": cancelled
        },
        "timestamp": now
    })))
}

// ── 3. GET /prd/{project_id} ────────────────────────────────────────────────

/// List all PRD files in .taskmaster/docs/.
async fn list_prd_files(
    Path(project_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    let docs_path = PathBuf::from(&project_path).join(".taskmaster").join("docs");

    if !docs_path.exists() {
        return Ok(Json(json!({
            "projectId": project_id,
            "prdFiles": [],
            "message": "No .taskmaster/docs directory found"
        })));
    }

    let mut entries = tokio::fs::read_dir(&docs_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to read PRD files",
                "message": e.to_string()
            })),
        )
    })?;

    let project_root = std::path::Path::new(&project_path);
    let mut prd_files: Vec<Value> = Vec::new();

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();

        // Only include .txt and .md files
        if !name.ends_with(".txt") && !name.ends_with(".md") {
            continue;
        }

        let is_file = entry.file_type().await.map(|ft| ft.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }

        let file_path = entry.path();
        let meta = tokio::fs::metadata(&file_path).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to read PRD file metadata",
                    "message": e.to_string()
                })),
            )
        })?;

        let relative = file_path
            .strip_prefix(project_root)
            .unwrap_or(&file_path)
            .to_string_lossy()
            .to_string();

        let modified = meta
            .modified()
            .ok()
            .map(|t| -> String {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            })
            .unwrap_or_default();

        let created = meta
            .created()
            .ok()
            .map(|t| -> String {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                dt.to_rfc3339()
            })
            .unwrap_or_default();

        prd_files.push(json!({
            "name": name,
            "path": relative,
            "size": meta.len(),
            "modified": modified,
            "created": created
        }));
    }

    // Sort by modified descending (newest first)
    prd_files.sort_by(|a, b| {
        b["modified"]
            .as_str()
            .unwrap_or("")
            .cmp(a["modified"].as_str().unwrap_or(""))
    });

    Ok(Json(json!({
        "projectId": project_id,
        "projectPath": project_path,
        "prdFiles": prd_files,
        "timestamp": now_iso()
    })))
}

// ── 4. POST /prd/{project_id} ───────────────────────────────────────────────

/// Create or update a PRD file in .taskmaster/docs/.
async fn create_prd_file(
    Path(project_id): Path<String>,
    Json(body): Json<CreatePrdBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    let docs_path = PathBuf::from(&project_path).join(".taskmaster").join("docs");

    // Validate filename: reject path traversal attempts
    if body.file_name.contains("..")
        || body.file_name.contains('/')
        || body.file_name.contains('\\')
        || body.file_name.starts_with('.')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid filename",
                "message": "Filename must not contain path separators, dots, or traversal sequences"
            })),
        ));
    }

    let file_path = docs_path.join(&body.file_name);

    // Validate filename: must end with .txt or .md
    if !body.file_name.ends_with(".txt") && !body.file_name.ends_with(".md") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid filename",
                "message": "Filename must end with .txt or .md"
            })),
        ));
    }

    // Ensure docs directory exists
    tokio::fs::create_dir_all(&docs_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to create docs directory",
                "message": e.to_string()
            })),
        )
    })?;

    // Write the PRD file
    tokio::fs::write(&file_path, &body.content)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to write PRD file",
                    "message": e.to_string()
                })),
            )
        })?;

    let meta = tokio::fs::metadata(&file_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to read PRD file metadata",
                "message": e.to_string()
            })),
        )
    })?;

    let created = meta
        .created()
        .ok()
        .map(|t| -> String {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        })
        .unwrap_or_default();

    let modified = meta
        .modified()
        .ok()
        .map(|t| -> String {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        })
        .unwrap_or_default();

    let relative = file_path
        .strip_prefix(std::path::Path::new(&project_path))
        .unwrap_or(&file_path)
        .to_string_lossy()
        .to_string();

    Ok(Json(json!({
        "projectId": project_id,
        "projectPath": project_path,
        "fileName": body.file_name,
        "filePath": relative,
        "size": meta.len(),
        "created": created,
        "modified": modified,
        "message": "PRD file saved successfully",
        "timestamp": now_iso()
    })))
}

// ── 5. GET /prd/{project_id}/{file_name} ────────────────────────────────────

/// Get content of a specific PRD file.
async fn get_prd_file(
    Path((project_id, file_name)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    let file_path = PathBuf::from(&project_path)
        .join(".taskmaster")
        .join("docs")
        .join(&file_name);

    if !file_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "PRD file not found",
                "message": format!("File \"{}\" does not exist", file_name)
            })),
        ));
    }

    let content = tokio::fs::read_to_string(&file_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to read PRD file",
                "message": e.to_string()
            })),
        )
    })?;

    let meta = tokio::fs::metadata(&file_path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to read PRD file metadata",
                "message": e.to_string()
            })),
        )
    })?;

    let relative = file_path
        .strip_prefix(std::path::Path::new(&project_path))
        .unwrap_or(&file_path)
        .to_string_lossy()
        .to_string();

    let created = meta
        .created()
        .ok()
        .map(|t| -> String {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        })
        .unwrap_or_default();

    let modified = meta
        .modified()
        .ok()
        .map(|t| -> String {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        })
        .unwrap_or_default();

    Ok(Json(json!({
        "projectId": project_id,
        "projectPath": project_path,
        "fileName": file_name,
        "filePath": relative,
        "content": content,
        "size": meta.len(),
        "created": created,
        "modified": modified,
        "timestamp": now_iso()
    })))
}

// ── 6. POST /parse-prd/{project_id} ─────────────────────────────────────────

/// Parse a PRD file to generate tasks by spawning npx task-master-ai.
async fn parse_prd(
    Path(project_id): Path<String>,
    Json(body): Json<ParsePrdBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    let file_name = body.file_name.unwrap_or_else(|| "prd.txt".to_string());
    let prd_path = PathBuf::from(&project_path)
        .join(".taskmaster")
        .join("docs")
        .join(&file_name);

    if !prd_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "PRD file not found",
                "message": format!("File \"{}\" does not exist in .taskmaster/docs/", file_name)
            })),
        ));
    }

    // Build command args as owned strings to avoid lifetime issues
    let mut all_args: Vec<String> = vec![
        "task-master-ai".to_string(),
        "parse-prd".to_string(),
        prd_path.to_string_lossy().to_string(),
    ];

    if let Some(num) = body.num_tasks {
        all_args.push("--num-tasks".to_string());
        all_args.push(num.to_string());
    }

    if body.append.unwrap_or(false) {
        all_args.push("--append".to_string());
    }

    all_args.push("--research".to_string());

    // Convert to &str slices for run_npx
    let arg_refs: Vec<&str> = all_args.iter().map(|s| s.as_str()).collect();

    // Run parse-prd command
    let (stdout, stderr, code) = run_npx(&arg_refs, &project_path, None)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to parse PRD",
                    "message": e
                })),
            )
        })?;

    match code {
        Some(0) => Ok(Json(json!({
            "projectId": project_id,
            "projectPath": project_path,
            "prdFile": file_name,
            "message": "PRD parsed and tasks generated successfully",
            "output": stdout,
            "timestamp": now_iso()
        }))),
        _ => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to parse PRD",
                "message": stderr.trim(),
                "code": code
            })),
        )),
    }
}

// ── 7. GET /prd-templates ───────────────────────────────────────────────────

/// Return built-in PRD templates.
async fn get_prd_templates() -> Json<Value> {
    Json(json!({
        "templates": prd_templates_list(),
        "timestamp": now_iso()
    }))
}

/// Build the list of PRD template objects with content filled in.
fn prd_templates_list() -> Vec<Value> {
    let date = today_date();

    vec![
        json!({
            "id": "web-app",
            "name": "Web Application",
            "description": "Template for web application projects with frontend and backend components",
            "category": "web",
            "content": WEB_APP_TEMPLATE.replace("[DATE]", &date)
        }),
        json!({
            "id": "api",
            "name": "REST API",
            "description": "Template for REST API development projects",
            "category": "backend",
            "content": API_TEMPLATE.replace("[DATE]", &date)
        }),
        json!({
            "id": "mobile-app",
            "name": "Mobile Application",
            "description": "Template for mobile app development projects (iOS/Android)",
            "category": "mobile",
            "content": MOBILE_APP_TEMPLATE.replace("[DATE]", &date)
        }),
        json!({
            "id": "data-analysis",
            "name": "Data Analysis Project",
            "description": "Template for data analysis and visualization projects",
            "category": "data",
            "content": DATA_ANALYSIS_TEMPLATE.replace("[DATE]", &date)
        }),
    ]
}

// ── 8. POST /apply-template/{project_id} ─────────────────────────────────────

/// Apply a PRD template to create a new PRD file.
async fn apply_template(
    Path(project_id): Path<String>,
    Json(body): Json<ApplyTemplateBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    let file_name = body.file_name.unwrap_or_else(|| "prd.txt".to_string());

    // Find the template
    let templates = prd_templates_list();
    let template = templates.iter().find(|t| t["id"].as_str() == Some(&body.template_id));

    let template = match template {
        Some(t) => t.clone(),
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Template not found",
                    "message": format!("Template \"{}\" does not exist", body.template_id)
                })),
            ));
        }
    };

    // Apply customizations by replacing [key] placeholders
    let mut content = template["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    if let Some(customizations) = body.customizations {
        for (key, value) in customizations {
            let placeholder = format!("[{}]", key);
            content = content.replace(&placeholder, &value);
        }
    }

    // Ensure docs directory exists
    let docs_dir = PathBuf::from(&project_path).join(".taskmaster").join("docs");
    tokio::fs::create_dir_all(&docs_dir)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to create docs directory",
                    "message": e.to_string()
                })),
            )
        })?;

    // Write the template content to the file
    let file_path = docs_dir.join(&file_name);
    tokio::fs::write(&file_path, &content).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to write PRD template",
                "message": e.to_string()
            })),
        )
    })?;

    Ok(Json(json!({
        "projectId": project_id,
        "projectPath": project_path,
        "templateId": body.template_id,
        "templateName": template["name"].as_str().unwrap_or(""),
        "fileName": file_name,
        "filePath": file_path.to_string_lossy().to_string(),
        "message": "PRD template applied successfully",
        "timestamp": now_iso()
    })))
}

// ── 9. POST /init/{project_id} ──────────────────────────────────────────────

/// Initialize TaskMaster in a project by spawning npx task-master init.
async fn init_taskmaster(
    Path(project_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    // Check if already initialized
    let taskmaster_path = PathBuf::from(&project_path).join(".taskmaster");
    if taskmaster_path.exists() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "TaskMaster already initialized",
                "message": "TaskMaster is already configured for this project"
            })),
        ));
    }

    // Run npx task-master init
    let (stdout, stderr, code) = run_npx(&["task-master", "init"], &project_path, Some(b"yes\n"))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to initialize TaskMaster",
                    "message": e
                })),
            )
        })?;

    match code {
        Some(0) => Ok(Json(json!({
            "projectId": project_id,
            "projectPath": project_path,
            "message": "TaskMaster initialized successfully",
            "output": stdout,
            "timestamp": now_iso()
        }))),
        _ => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to initialize TaskMaster",
                "message": stderr.trim(),
                "code": code
            })),
        )),
    }
}

// ── 10. POST /add-task/{project_id} ─────────────────────────────────────────

/// Add a new task via npx task-master-ai add-task.
async fn add_task(
    Path(project_id): Path<String>,
    Json(body): Json<AddTaskBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    // Validate: either prompt, or both title and description
    let has_prompt = body.prompt.is_some();
    let has_title_desc = body.title.is_some() && body.description.is_some();

    if !has_prompt && !has_title_desc {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Missing required parameters",
                "message": "Either \"prompt\" or both \"title\" and \"description\" are required"
            })),
        ));
    }

    // Build args
    let mut args: Vec<String> = vec!["task-master-ai".to_string(), "add-task".to_string()];

    if let Some(prompt) = body.prompt {
        args.push("--prompt".to_string());
        args.push(prompt);
        args.push("--research".to_string());
    } else if let (Some(title), Some(description)) = (body.title, body.description) {
        let prompt = format!("Create a task titled \"{}\" with description: {}", title, description);
        args.push("--prompt".to_string());
        args.push(prompt);
    }

    if let Some(priority) = body.priority {
        args.push("--priority".to_string());
        args.push(priority);
    }

    if let Some(dependencies) = body.dependencies {
        args.push("--dependencies".to_string());
        args.push(dependencies);
    }

    // Convert args to &str slices for run_npx
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let (stdout, stderr, code) = run_npx(&arg_refs, &project_path, None)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to add task",
                    "message": e
                })),
            )
        })?;

    match code {
        Some(0) => Ok(Json(json!({
            "projectId": project_id,
            "projectPath": project_path,
            "message": "Task added successfully",
            "output": stdout,
            "timestamp": now_iso()
        }))),
        _ => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to add task",
                "message": stderr.trim(),
                "code": code
            })),
        )),
    }
}

// ── 11. PUT /update-task/{project_id}/{task_id} ──────────────────────────────

/// Update a specific task using npx task-master-ai set-status or update-task.
async fn update_task(
    Path((project_id, task_id)): Path<(String, String)>,
    Json(body): Json<UpdateTaskBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = resolve_project(&project_id)?;

    // If only updating status, use the faster set-status command
    if let Some(ref status) = body.status {
        // Check if ONLY status is being updated (no other fields)
        let has_other_fields = body.title.is_some()
            || body.description.is_some()
            || body.priority.is_some()
            || body.details.is_some();

        if !has_other_fields {
            let id_arg = format!("--id={}", task_id);
            let status_arg = format!("--status={}", status);
            let args = ["task-master-ai", "set-status", &id_arg, &status_arg];

            let (stdout, stderr, code) = run_npx(&args, &project_path, None)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "Failed to update task status",
                            "message": e
                        })),
                    )
                })?;

            return match code {
                Some(0) => Ok(Json(json!({
                    "projectId": project_id,
                    "projectPath": project_path,
                    "taskId": task_id,
                    "message": "Task status updated successfully",
                    "output": stdout,
                    "timestamp": now_iso()
                }))),
                _ => Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Failed to update task status",
                        "message": stderr.trim(),
                        "code": code
                    })),
                )),
            };
        }
    }

    // For general updates, use the update-task command with a prompt
    let mut updates: Vec<String> = Vec::new();
    if let Some(ref title) = body.title {
        updates.push(format!("title: \"{}\"", title));
    }
    if let Some(ref description) = body.description {
        updates.push(format!("description: \"{}\"", description));
    }
    if let Some(ref priority) = body.priority {
        updates.push(format!("priority: \"{}\"", priority));
    }
    if let Some(ref details) = body.details {
        updates.push(format!("details: \"{}\"", details));
    }

    let prompt = format!("Update task with the following changes: {}", updates.join(", "));
    let id_arg = format!("--id={}", task_id);
    let prompt_arg = format!("--prompt={}", prompt);

    let args = ["task-master-ai", "update-task", &id_arg, &prompt_arg];

    let (stdout, stderr, code) = run_npx(&args, &project_path, None)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to update task",
                    "message": e
                })),
            )
        })?;

    match code {
        Some(0) => Ok(Json(json!({
            "projectId": project_id,
            "projectPath": project_path,
            "taskId": task_id,
            "message": "Task updated successfully",
            "output": stdout,
            "timestamp": now_iso()
        }))),
        _ => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to update task",
                "message": stderr.trim(),
                "code": code
            })),
        )),
    }
}

// ── PRD Template Constants ───────────────────────────────────────────────────

const WEB_APP_TEMPLATE: &str = r#"# Product Requirements Document - Web Application

## Overview
**Product Name:** [Your App Name]
**Version:** 1.0
**Date:** [DATE]
**Author:** [Your Name]

## Executive Summary
Brief description of what this web application will do and why it's needed.

## Product Goals
- Goal 1: [Specific measurable goal]
- Goal 2: [Specific measurable goal]
- Goal 3: [Specific measurable goal]

## User Stories
### Core Features
1. **User Registration & Authentication**
   - As a user, I want to create an account so I can access personalized features
   - As a user, I want to log in securely so my data is protected
   - As a user, I want to reset my password if I forget it

2. **Main Application Features**
   - As a user, I want to [core feature 1] so I can [benefit]
   - As a user, I want to [core feature 2] so I can [benefit]
   - As a user, I want to [core feature 3] so I can [benefit]

3. **User Interface**
   - As a user, I want a responsive design so I can use the app on any device
   - As a user, I want intuitive navigation so I can easily find features

## Technical Requirements
### Frontend
- Framework: React/Vue/Angular or vanilla JavaScript
- Styling: CSS framework (Tailwind, Bootstrap, etc.)
- State Management: Redux/Vuex/Context API
- Build Tools: Webpack/Vite
- Testing: Jest/Vitest for unit tests

### Backend
- Runtime: Node.js/Python/Java
- Database: PostgreSQL/MySQL/MongoDB
- API: RESTful API or GraphQL
- Authentication: JWT tokens
- Testing: Integration and unit tests

### Infrastructure
- Hosting: Cloud provider (AWS, Azure, GCP)
- CI/CD: GitHub Actions/GitLab CI
- Monitoring: Application monitoring tools
- Security: HTTPS, input validation, rate limiting

## Success Metrics
- User engagement metrics
- Performance benchmarks (load time < 2s)
- Error rates < 1%
- User satisfaction scores

## Timeline
- Phase 1: Core functionality (4-6 weeks)
- Phase 2: Advanced features (2-4 weeks)
- Phase 3: Polish and launch (2 weeks)

## Constraints & Assumptions
- Budget constraints
- Technical limitations
- Team size and expertise
- Timeline constraints"#;

const API_TEMPLATE: &str = r#"# Product Requirements Document - REST API

## Overview
**API Name:** [Your API Name]
**Version:** v1.0
**Date:** [DATE]
**Author:** [Your Name]

## Executive Summary
Description of the API's purpose, target users, and primary use cases.

## API Goals
- Goal 1: Provide secure data access
- Goal 2: Ensure scalable architecture
- Goal 3: Maintain high availability (99.9% uptime)

## Functional Requirements
### Core Endpoints
1. **Authentication Endpoints**
   - POST /api/auth/login - User authentication
   - POST /api/auth/logout - User logout
   - POST /api/auth/refresh - Token refresh
   - POST /api/auth/register - User registration

2. **Data Management Endpoints**
   - GET /api/resources - List resources with pagination
   - GET /api/resources/{id} - Get specific resource
   - POST /api/resources - Create new resource
   - PUT /api/resources/{id} - Update existing resource
   - DELETE /api/resources/{id} - Delete resource

3. **Administrative Endpoints**
   - GET /api/admin/users - Manage users (admin only)
   - GET /api/admin/analytics - System analytics
   - POST /api/admin/backup - Trigger system backup

## Technical Requirements
### API Design
- RESTful architecture following OpenAPI 3.0 specification
- JSON request/response format
- Consistent error response format
- API versioning strategy

### Authentication & Security
- JWT token-based authentication
- Role-based access control (RBAC)
- Rate limiting (100 requests/minute per user)
- Input validation and sanitization
- HTTPS enforcement

### Database
- Database type: [PostgreSQL/MongoDB/MySQL]
- Connection pooling
- Database migrations
- Backup and recovery procedures

### Performance Requirements
- Response time: < 200ms for 95% of requests
- Throughput: 1000+ requests/second
- Concurrent users: 10,000+
- Database query optimization

### Documentation
- Auto-generated API documentation (Swagger/OpenAPI)
- Code examples for common use cases
- SDK development for major languages
- Postman collection for testing

## Error Handling
- Standardized error codes and messages
- Proper HTTP status codes
- Detailed error logging
- Graceful degradation strategies

## Testing Strategy
- Unit tests (80%+ coverage)
- Integration tests for all endpoints
- Load testing and performance testing
- Security testing (OWASP compliance)

## Monitoring & Logging
- Application performance monitoring
- Error tracking and alerting
- Access logs and audit trails
- Health check endpoints

## Deployment
- Containerized deployment (Docker)
- CI/CD pipeline setup
- Environment management (dev, staging, prod)
- Blue-green deployment strategy

## Success Metrics
- API uptime > 99.9%
- Average response time < 200ms
- Zero critical security vulnerabilities
- Developer adoption metrics"#;

const MOBILE_APP_TEMPLATE: &str = r#"# Product Requirements Document - Mobile Application

## Overview
**App Name:** [Your App Name]
**Platform:** iOS / Android / Cross-platform
**Version:** 1.0
**Date:** [DATE]
**Author:** [Your Name]

## Executive Summary
Brief description of the mobile app's purpose, target audience, and key value proposition.

## Product Goals
- Goal 1: [Specific user engagement goal]
- Goal 2: [Specific functionality goal]
- Goal 3: [Specific performance goal]

## User Stories
### Core Features
1. **Onboarding & Authentication**
   - As a new user, I want a simple onboarding process
   - As a user, I want to sign up with email or social media
   - As a user, I want biometric authentication for security

2. **Main App Features**
   - As a user, I want [core feature 1] accessible from home screen
   - As a user, I want [core feature 2] to work offline
   - As a user, I want to sync data across devices

3. **User Experience**
   - As a user, I want intuitive navigation patterns
   - As a user, I want fast loading times
   - As a user, I want accessibility features

## Technical Requirements
### Mobile Development
- **Cross-platform:** React Native / Flutter / Xamarin
- **Native:** Swift (iOS) / Kotlin (Android)
- **State Management:** Redux / MobX / Provider
- **Navigation:** React Navigation / Flutter Navigation

### Backend Integration
- REST API or GraphQL integration
- Real-time features (WebSockets/Push notifications)
- Offline data synchronization
- Background processing

### Device Features
- Camera and photo library access
- GPS location services
- Push notifications
- Biometric authentication
- Device storage

### Performance Requirements
- App launch time < 3 seconds
- Screen transition animations < 300ms
- Memory usage optimization
- Battery usage optimization

## Platform-Specific Considerations
### iOS Requirements
- iOS 13.0+ minimum version
- App Store guidelines compliance
- iOS design guidelines (Human Interface Guidelines)
- TestFlight beta testing

### Android Requirements
- Android 8.0+ (API level 26) minimum
- Google Play Store guidelines
- Material Design guidelines
- Google Play Console testing

## User Interface Design
- Responsive design for different screen sizes
- Dark mode support
- Accessibility compliance (WCAG 2.1)
- Consistent design system

## Security & Privacy
- Secure data storage (Keychain/Keystore)
- API communication encryption
- Privacy policy compliance (GDPR/CCPA)
- App security best practices

## Testing Strategy
- Unit testing (80%+ coverage)
- UI/E2E testing (Detox/Appium)
- Device testing on multiple screen sizes
- Performance testing
- Security testing

## App Store Deployment
- App store optimization (ASO)
- App icons and screenshots
- Store listing content
- Release management strategy

## Analytics & Monitoring
- User analytics (Firebase/Analytics)
- Crash reporting (Crashlytics/Sentry)
- Performance monitoring
- User feedback collection

## Success Metrics
- App store ratings > 4.0
- User retention rates
- Daily/Monthly active users
- App performance metrics
- Conversion rates"#;

const DATA_ANALYSIS_TEMPLATE: &str = r#"# Product Requirements Document - Data Analysis Project

## Overview
**Project Name:** [Your Analysis Project]
**Analysis Type:** [Descriptive/Predictive/Prescriptive]
**Date:** [DATE]
**Author:** [Your Name]

## Executive Summary
Description of the business problem, data sources, and expected insights.

## Project Goals
- Goal 1: [Specific business question to answer]
- Goal 2: [Specific prediction to make]
- Goal 3: [Specific recommendation to provide]

## Business Requirements
### Key Questions
1. What patterns exist in the current data?
2. What factors influence [target variable]?
3. What predictions can be made for [future outcome]?
4. What recommendations can improve [business metric]?

### Success Criteria
- Actionable insights for stakeholders
- Statistical significance in findings
- Reproducible analysis pipeline
- Clear visualization and reporting

## Data Requirements
### Data Sources
1. **Primary Data**
   - Source: [Database/API/Files]
   - Format: [CSV/JSON/SQL]
   - Size: [Volume estimate]
   - Update frequency: [Real-time/Daily/Monthly]

2. **External Data**
   - Third-party APIs
   - Public datasets
   - Market research data

### Data Quality Requirements
- Data completeness (< 5% missing values)
- Data accuracy validation
- Data consistency checks
- Historical data availability

## Technical Requirements
### Data Pipeline
- Data extraction and ingestion
- Data cleaning and preprocessing
- Data transformation and feature engineering
- Data validation and quality checks

### Analysis Tools
- **Programming:** Python/R/SQL
- **Libraries:** pandas, numpy, scikit-learn, matplotlib
- **Visualization:** Tableau, PowerBI, or custom dashboards
- **Version Control:** Git for code and DVC for data

### Computing Resources
- Local development environment
- Cloud computing (AWS/GCP/Azure) if needed
- Database access and permissions
- Storage requirements

## Analysis Methodology
### Data Exploration
1. Descriptive statistics and data profiling
2. Data visualization and pattern identification
3. Correlation analysis
4. Outlier detection and handling

### Statistical Analysis
1. Hypothesis formulation
2. Statistical testing
3. Confidence intervals
4. Effect size calculations

### Machine Learning (if applicable)
1. Feature selection and engineering
2. Model selection and training
3. Cross-validation and evaluation
4. Model interpretation and explainability

## Deliverables
### Reports
- Executive summary for stakeholders
- Technical analysis report
- Data quality report
- Methodology documentation

### Visualizations
- Interactive dashboards
- Static charts and graphs
- Data story presentations
- Key findings infographics

### Code & Documentation
- Reproducible analysis scripts
- Data pipeline code
- Documentation and comments
- Testing and validation code

## Timeline
- Phase 1: Data collection and exploration (2 weeks)
- Phase 2: Analysis and modeling (3 weeks)
- Phase 3: Reporting and visualization (1 week)
- Phase 4: Stakeholder presentation (1 week)

## Risks & Assumptions
- Data availability and quality risks
- Technical complexity assumptions
- Resource and timeline constraints
- Stakeholder engagement assumptions

## Success Metrics
- Stakeholder satisfaction with insights
- Accuracy of predictions (if applicable)
- Business impact of recommendations
- Reproducibility of results"#;
