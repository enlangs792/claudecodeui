//! Cursor Agent — mirrors server/cursor-cli.js
//!
//! Spawns the `cursor-agent` CLI as a subprocess with --output-format stream-json.
//! Handles workspace trust prompts and session management.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

static ACTIVE_SESSIONS: LazyLock<Mutex<HashMap<String, tokio::process::Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const WORKSPACE_TRUST_PATTERNS: &[&str] = &[
    "workspace trust required",
    "do you trust the contents of this directory",
    "working with untrusted contents",
    "pass --trust",
];

fn is_workspace_trust_prompt(text: &str) -> bool {
    let lower = text.to_lowercase();
    WORKSPACE_TRUST_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

pub struct CursorOptions {
    pub session_id: Option<String>,
    pub project_path: String,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub model: Option<String>,
    pub skip_permissions: bool,
    pub tools_settings: Option<super::claude_agent::ToolsSettings>,
}

/// Spawn a Cursor CLI process and stream output via callbacks.
pub async fn spawn_cursor(
    options: CursorOptions,
    on_message: impl Fn(&str) + Send + Sync + 'static,
    on_error: impl Fn(&str) + Send + Sync + 'static,
) -> Result<String, String> {
    let session_id = options
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut args: Vec<String> = Vec::new();

    // Resume existing session
    if options.session_id.is_some() {
        args.push(format!("--resume={}", session_id));
    }

    // Add prompt
    if let Some(ref cmd) = options.command {
        if !cmd.trim().is_empty() {
            args.push("-p".into());
            args.push(cmd.clone());
        }
    }

    // Model
    if options.session_id.is_none() {
        if let Some(ref model) = options.model {
            args.push("--model".into());
            args.push(model.clone());
        }
    }

    // Streaming output
    if options.command.is_some() {
        args.push("--output-format".into());
        args.push("stream-json".into());
    }

    // Skip permissions
    if options.skip_permissions {
        args.push("-f".into());
    }

    let working_dir = options.cwd.unwrap_or_else(|| options.project_path.clone());

    let mut child = Command::new("cursor-agent")
        .args(&args)
        .current_dir(&working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn cursor-agent: {e}"))?;

    let stdout = child.stdout.take().ok_or("No stdout")?;
    let stderr = child.stderr.take().ok_or("No stderr")?;

    {
        let mut sessions = ACTIVE_SESSIONS.lock().map_err(|e| format!("Lock error: {e}"))?;
        sessions.insert(session_id.clone(), child);
    }

    let stdout_reader = tokio::io::BufReader::new(stdout);
    let mut stdout_lines = stdout_reader.lines();
    let stderr_reader = tokio::io::BufReader::new(stderr);
    let mut stderr_lines = stderr_reader.lines();

    let sid = session_id.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = stdout_lines.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            if !line.trim().is_empty() {
                                // Auto-retry with --trust on workspace trust prompt
                                if is_workspace_trust_prompt(&line) {
                                    // Signal frontend that trust is needed
                                    on_message(&format!(
                                        r#"{{"type":"workspace_trust_required","message":"{}"}}"#,
                                        line.replace('"', "'")
                                    ));
                                } else {
                                    on_message(&line);
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
                result = stderr_lines.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            if !line.trim().is_empty() {
                                on_error(&line);
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }
        if let Ok(mut sessions) = ACTIVE_SESSIONS.lock() {
            sessions.remove(&sid);
        }
    });

    Ok(session_id)
}

/// Abort an active Cursor session.
pub fn abort_cursor_session(session_id: &str) -> Result<(), String> {
    let mut sessions = ACTIVE_SESSIONS.lock().map_err(|e| format!("Lock error: {e}"))?;
    if let Some(mut child) = sessions.remove(session_id) {
        let _ = child.start_kill();
        let _ = child.try_wait();
    }
    Ok(())
}

/// Check if a Cursor session is active.
pub fn is_cursor_session_active(session_id: &str) -> bool {
    ACTIVE_SESSIONS
        .lock()
        .map(|sessions| sessions.contains_key(session_id))
        .unwrap_or(false)
}

/// Get all active Cursor session IDs.
pub fn get_active_cursor_sessions() -> Vec<String> {
    ACTIVE_SESSIONS
        .lock()
        .map(|sessions| sessions.keys().cloned().collect())
        .unwrap_or_default()
}
