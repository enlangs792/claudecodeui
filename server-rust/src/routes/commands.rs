//! Commands route — mirrors server/routes/commands.js + utils/commandParser.js
//!
//! POST /list    — list available commands (built-in + custom from ~/.claude/commands/)
//! POST /execute — execute a command by name (built-in or custom)

use axum::{
    Extension,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command as StdCommand;

use crate::auth::middleware::AuthUser;
use crate::shared::model_constants;

pub fn routes() -> Router {
    Router::new()
        .route("/list", post(list_commands))
        .route("/execute", post(execute_command))
}

// ── Built-in command definitions ───────────────────────────────────────────

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

// ── Custom command scanning ─────────────────────────────────────────────────

fn commands_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claude")
        .join("commands")
}

/// Parse frontmatter from a markdown file to extract command metadata.
fn parse_command_metadata(content: &str) -> Option<Value> {
    let trimmed = content.trim();
    // Look for YAML frontmatter delimited by ---
    if let Some(rest) = trimmed.strip_prefix("---") {
        if let Some(end_idx) = rest.find("---") {
            let frontmatter = &rest[..end_idx];
            let mut name = String::new();
            let mut description = String::new();
            let mut args_str = String::new();

            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(value) = line.strip_prefix("name:") {
                    name = value.trim().to_string();
                } else if let Some(value) = line.strip_prefix("description:") {
                    description = value.trim().to_string();
                } else if let Some(value) = line.strip_prefix("args:") {
                    args_str = value.trim().to_string();
                }
            }

            if !name.is_empty() {
                let args: Vec<Value> = if !args_str.is_empty() {
                    vec![json!({"name": args_str, "description": format!("Argument for {}", name)})]
                } else {
                    vec![]
                };

                let body = rest[end_idx + 3..].trim().to_string();

                return Some(json!({
                    "name": name,
                    "description": description,
                    "namespace": "custom",
                    "metadata": {
                        "type": "custom",
                        "args": args,
                    },
                    "body": body,
                }));
            }
        }
    }
    None
}

fn scan_custom_commands() -> Vec<Value> {
    let dir = commands_dir();
    let mut commands = Vec::new();

    if !dir.exists() {
        return commands;
    }

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "md") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Some(cmd) = parse_command_metadata(&content) {
                        commands.push(cmd);
                    }
                }
            }
        }
    }

    commands
}

// ── Argument replacement ────────────────────────────────────────────────────

fn replace_arguments(content: &str, args: &[String]) -> String {
    let mut result = content.to_string();

    // Replace $ARGUMENTS with all args joined
    let all_args = args.join(" ");
    result = result.replace("$ARGUMENTS", &all_args);

    // Replace $1..$9 with individual args
    for (i, arg) in args.iter().enumerate() {
        if i < 9 {
            let placeholder = format!("${}", i + 1);
            result = result.replace(&placeholder, arg);
        }
    }

    result
}

// ── File includes ───────────────────────────────────────────────────────────

fn process_file_includes(content: &str, base_path: &std::path::Path, depth: u32) -> String {
    if depth > 3 {
        return content.to_string();
    }

    let mut result = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(filename) = trimmed.strip_prefix('@') {
            let include_path = base_path.join(filename);
            if let Ok(included) = std::fs::read_to_string(&include_path) {
                result.push_str(&included);
                result.push('\n');
            } else {
                result.push_str(line);
                result.push('\n');
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

// ── Bash execution ──────────────────────────────────────────────────────────

const ALLOWED_COMMANDS: &[&str] = &[
    "echo", "ls", "pwd", "date", "whoami", "git", "npm", "node", "cat", "grep", "find",
];

fn validate_bash_command(cmd: &str) -> bool {
    let first_word = cmd.split_whitespace().next().unwrap_or("");
    ALLOWED_COMMANDS.contains(&first_word)
}

fn process_bash_commands(content: &str) -> String {
    let mut result = String::new();
    for line in content.lines() {
        if let Some(cmd) = line.trim().strip_prefix('!') {
            if !validate_bash_command(cmd) {
                result.push_str(&format!("[Blocked unsafe command: {cmd}]\n"));
                continue;
            }
            match StdCommand::new("sh")
                .args(["-c", cmd])
                .output()
            {
                Ok(output) => {
                    result.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                Err(e) => {
                    result.push_str(&format!("[Error executing command: {e}]\n"));
                }
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

// ── Request shapes ──────────────────────────────────────────────────────────

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

// ── Handlers ────────────────────────────────────────────────────────────────

/// POST /api/commands/list
async fn list_commands(
    Extension(_user): Extension<AuthUser>,
    Json(_body): Json<ListCommandsBody>,
) -> Json<Value> {
    let builtin = built_in_commands();
    let custom = scan_custom_commands();
    let total = builtin.len() + custom.len();

    Json(json!({
        "builtIn": builtin,
        "custom": custom,
        "count": total
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

    // Check built-in commands first
    match command {
        "/help" => {
            let mut lines: Vec<String> = built_in_commands().iter().map(|cmd| {
                format!("### {}\n{}", cmd["name"], cmd["description"])
            }).collect();

            // Add custom commands to help
            let custom = scan_custom_commands();
            if !custom.is_empty() {
                lines.push("\n## Custom Commands\n".into());
                for cmd in &custom {
                    lines.push(format!("### {}\n{}", cmd["name"], cmd["description"]));
                }
            }

            let help_text = format!(
                "# Claude Code Commands\n\n## Built-in Commands\n\n{}",
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
            // Look for custom command match
            let custom_commands = scan_custom_commands();
            for cmd in &custom_commands {
                if cmd["name"].as_str() == Some(command) {
                    let body_content = cmd["body"].as_str().unwrap_or("");
                    let base_path = commands_dir();

                    // Process the command content
                    let processed = replace_arguments(body_content, &body.args);
                    let with_includes = process_file_includes(&processed, &base_path, 0);
                    let result = process_bash_commands(&with_includes);

                    return Ok(Json(json!({
                        "type": "custom",
                        "command": command,
                        "action": "execute",
                        "data": {
                            "content": result,
                            "format": "markdown"
                        }
                    })));
                }
            }

            // Unknown command
            Ok(Json(json!({
                "type": "unknown",
                "command": command,
                "data": {
                    "message": format!("Command '{}' is not recognized.", command)
                }
            })))
        }
    }
}
