//! Codex Agent — mirrors server/openai-codex.js
//!
//! Spawns the `codex` CLI as a subprocess with streaming output.
//! Provides session management (spawn, abort, status).

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

static ACTIVE_SESSIONS: LazyLock<Mutex<HashMap<String, tokio::process::Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct CodexOptions {
    pub session_id: Option<String>,
    pub project_path: String,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub sandbox_mode: Option<String>,
}

/// Spawn a Codex CLI process and stream output via callbacks.
pub async fn spawn_codex(
    options: CodexOptions,
    on_message: impl Fn(&str) + Send + Sync + 'static,
    on_error: impl Fn(&str) + Send + Sync + 'static,
) -> Result<String, String> {
    let session_id = options
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut args: Vec<String> = Vec::new();

    // Resume existing session
    if let Some(ref sid) = options.session_id {
        args.push("--resume".into());
        args.push(sid.clone());
    }

    // Add prompt
    if let Some(ref cmd) = options.command {
        if !cmd.trim().is_empty() {
            args.push("-p".into());
            args.push(cmd.clone());
        }
    }

    // Model
    if let Some(ref model) = options.model {
        args.push("--model".into());
        args.push(model.clone());
    }

    // Sandbox mode
    if let Some(ref mode) = options.sandbox_mode {
        args.push("--sandbox".into());
        args.push(mode.clone());
    }

    // Permission mode
    if let Some(ref mode) = options.permission_mode {
        args.push("--permission-mode".into());
        args.push(mode.clone());
    }

    // Streaming output
    args.push("--output-format".into());
    args.push("stream-json".into());

    let working_dir = options.cwd.unwrap_or_else(|| options.project_path.clone());

    let mut child = Command::new("codex")
        .args(&args)
        .current_dir(&working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn codex: {e}"))?;

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
                                on_message(&line);
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

/// Abort an active Codex session.
pub fn abort_codex_session(session_id: &str) -> Result<(), String> {
    let mut sessions = ACTIVE_SESSIONS.lock().map_err(|e| format!("Lock error: {e}"))?;
    if let Some(mut child) = sessions.remove(session_id) {
        let _ = child.start_kill();
        let _ = child.try_wait();
    }
    Ok(())
}

/// Check if a Codex session is active.
pub fn is_codex_session_active(session_id: &str) -> bool {
    ACTIVE_SESSIONS
        .lock()
        .map(|sessions| sessions.contains_key(session_id))
        .unwrap_or(false)
}

/// Get all active Codex session IDs.
pub fn get_active_codex_sessions() -> Vec<String> {
    ACTIVE_SESSIONS
        .lock()
        .map(|sessions| sessions.keys().cloned().collect())
        .unwrap_or_default()
}
