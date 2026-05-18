//! Commands route — mirrors server/routes/commands.js
//!
//! POST /list    — list available commands (built-in + custom)
//! POST /execute — execute a command by name

use axum::{
    Extension,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::shared::model_constants;

pub fn routes() -> Router {
    Router::new()
        .route("/list", post(list_commands))
        .route("/execute", post(execute_command))
}

// ── Built-in command definitions (mirrors builtInCommands in commands.js) ─────

fn built_in_commands() -> Vec<Value> {
    vec![
        json!({"name": "/help", "description": "Show help documentation for Claude Code", "namespace": "builtin", "metadata": {"type": "builtin"}}),
        json!({"name": "/clear", "description": "Clear the conversation history", "namespace": "builtin", "metadata": {"type": "builtin"}}),
        json!({"name": "/model", "description": "Switch or view the current AI model", "namespace": "builtin", "metadata": {"type": "builtin"}}),
        json!({"name": "/cost", "description": "Display token usage and cost information", "namespace": "builtin", "metadata": {"type": "builtin"}}),
        json!({"name": "/memory", "description": "Open CLAUDE.md memory file for editing", "namespace": "builtin", "metadata": {"type": "builtin"}}),
        json!({"name": "/config", "description": "Open settings and configuration", "namespace": "builtin", "metadata": {"type": "builtin"}}),
        json!({"name": "/status", "description": "Show system status and version information", "namespace": "builtin", "metadata": {"type": "builtin"}}),
        json!({"name": "/rewind", "description": "Rewind the conversation to a previous state", "namespace": "builtin", "metadata": {"type": "builtin"}}),
    ]
}

// ── Request shapes ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ListCommandsBody {
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExecuteCommandBody {
    /// Command name (e.g. "/help")
    command: Option<String>,
    /// Command arguments
    #[serde(default)]
    args: Vec<String>,
    /// Project path (from body; also accepted as context.projectPath)
    project: Option<String>,
    /// Session identifier
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    /// Capture remaining fields for forward-compat
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/commands/list
async fn list_commands(
    Extension(_user): Extension<AuthUser>,
    Json(_body): Json<ListCommandsBody>,
) -> Json<Value> {
    let builtin = built_in_commands();
    // No filesystem scanning for .claude/commands/ in the Rust backend yet.
    // Custom commands will be returned once directory scanning is added.
    let custom: Vec<Value> = Vec::new();

    Json(json!({
        "builtIn": builtin,
        "custom": custom,
        "count": builtin.len()
    }))
}

/// POST /api/commands/execute
async fn execute_command(
    Extension(_user): Extension<AuthUser>,
    Json(body): Json<ExecuteCommandBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let command = body.command.as_deref().unwrap_or("");

    if command.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Command name is required"})),
        ));
    }

    match command {
        "/help" => {
            let lines: Vec<String> = built_in_commands().iter().map(|cmd| {
                format!("### {}\n{}", cmd["name"], cmd["description"])
            }).collect();

            let help_text = format!(
                "# Claude Code Commands\n\n## Built-in Commands\n\n{}\n\n## Custom Commands\n\nCustom commands are not yet supported in the Rust backend.",
                lines.join("\n")
            );

            Ok(Json(json!({
                "type": "builtin",
                "command": command,
                "action": "help",
                "data": {
                    "content": help_text,
                    "format": "markdown"
                }
            })))
        }

        "/clear" => Ok(Json(json!({
            "type": "builtin",
            "command": command,
            "action": "clear",
            "data": {
                "message": "Conversation history cleared"
            }
        }))),

        "/model" => {
            let providers = model_constants::providers();
            let available: Value = providers.iter().map(|p| {
                (p.id.clone(), json!({
                    "options": p.models.options.iter().map(|m| m.value.clone()).collect::<Vec<_>>(),
                    "default": p.models.default
                }))
            }).collect();

            Ok(Json(json!({
                "type": "builtin",
                "command": command,
                "action": "model",
                "data": {
                    "current": {
                        "provider": "claude",
                        "model": model_constants::claude_models().default
                    },
                    "available": available,
                    "message": if body.args.is_empty() {
                        format!("Current model: {}", model_constants::claude_models().default)
                    } else {
                        format!("Switching to model: {}", body.args[0])
                    }
                }
            })))
        }

        "/cost" => Ok(Json(json!({
            "type": "builtin",
            "command": command,
            "action": "cost",
            "data": {
                "tokenUsage": {
                    "used": 0,
                    "total": 160000,
                    "percentage": 0.0
                },
                "cost": {
                    "input": "0.0000",
                    "output": "0.0000",
                    "total": "0.0000"
                },
                "model": model_constants::claude_models().default
            }
        }))),

        "/status" => Ok(Json(json!({
            "type": "builtin",
            "command": command,
            "action": "status",
            "data": {
                "version": env!("CARGO_PKG_VERSION"),
                "packageName": "claude-code-ui",
                "uptime": "0m",
                "uptimeSeconds": 0,
                "model": model_constants::claude_models().default,
                "provider": "claude",
                "nodeVersion": "rust",
                "platform": std::env::consts::OS
            }
        }))),

        "/memory" => {
            let project_path = body.project.as_deref().unwrap_or("");
            let has_project = !project_path.is_empty();

            Ok(Json(json!({
                "type": "builtin",
                "command": command,
                "action": "memory",
                "data": {
                    "path": if has_project { format!("{}/CLAUDE.md", project_path) } else { String::new() },
                    "exists": false,
                    "message": if has_project {
                        format!("Opening CLAUDE.md at {}/CLAUDE.md", project_path)
                    } else {
                        "No project selected. Please select a project to access its CLAUDE.md file.".to_string()
                    }
                }
            })))
        }

        "/config" => Ok(Json(json!({
            "type": "builtin",
            "command": command,
            "action": "config",
            "data": {
                "message": "Opening settings..."
            }
        }))),

        "/rewind" => {
            let steps = body.args.first()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);

            Ok(Json(json!({
                "type": "builtin",
                "command": command,
                "action": "rewind",
                "data": {
                    "steps": steps,
                    "message": format!("Rewinding conversation by {} step{}...", steps, if steps > 1 { "s" } else { "" })
                }
            })))
        }

        _ => {
            // Unknown command — return a stub response
            Ok(Json(json!({
                "type": "unknown",
                "command": command,
                "data": {
                    "message": format!("Command '{}' is not yet handled by the Rust backend.", command)
                }
            })))
        }
    }
}
