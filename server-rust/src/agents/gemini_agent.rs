//! Gemini Agent — mirrors server/gemini-cli.js
//!
//! Spawns the `gemini` CLI as a subprocess via shell wrapper.
//! Handles NDJSON output parsing and exit code mapping.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

static ACTIVE_SESSIONS: LazyLock<Mutex<HashMap<String, tokio::process::Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Map Gemini exit codes to user-friendly messages (mirrors mapGeminiExitCodeToMessage)
fn map_exit_code(code: Option<i32>) -> Option<&'static str> {
    match code {
        Some(41) => Some("Gemini authentication error (exit code 41). Check your API key."),
        Some(42) => Some("Gemini rejected the request input (exit code 42)."),
        Some(44) => Some("Gemini sandbox error (exit code 44). Check local sandbox/container settings."),
        Some(52) => Some("Gemini configuration error (exit code 52). Check your Gemini settings files for invalid JSON/config."),
        Some(53) => Some("Gemini conversation turn limit reached (exit code 53). Start a new Gemini session."),
        _ => None,
    }
}

pub struct GeminiOptions {
    pub session_id: Option<String>,
    pub project_path: String,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub model: Option<String>,
    pub attach_images: Option<Vec<String>>,
    pub env_vars: HashMap<String, String>,
}

/// Spawn a Gemini CLI process and stream output via callbacks.
pub async fn spawn_gemini(
    options: GeminiOptions,
    on_message: impl Fn(&str) + Send + Sync + 'static,
    on_error: impl Fn(&str) + Send + Sync + 'static,
) -> Result<String, String> {
    let session_id = options
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut args: Vec<String> = Vec::new();

    // Handle image attachments
    if let Some(ref images) = options.attach_images {
        for img in images {
            args.push("--image".into());
            args.push(img.clone());
        }
    }

    // Session resume
    if let Some(ref sid) = options.session_id {
        args.push("--resume".into());
        args.push(sid.clone());
    }

    // Model
    if let Some(ref model) = options.model {
        args.push("--model".into());
        args.push(model.clone());
    }

    // Use shell wrapper for shebang support (mirrors TS behavior)
    if let Some(ref prompt) = options.command {
        if !prompt.trim().is_empty() {
            args.push("-p".into());
            args.push(prompt.clone());
        }
    }

    let working_dir = options.cwd.unwrap_or_else(|| options.project_path.clone());

    // Build environment with user's .env for auth keys
    let mut cmd = Command::new("sh");
    cmd.args(["-c", &format!("exec gemini {}", args.join(" "))])
        .current_dir(&working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Inject auth env vars
    for (key, value) in &options.env_vars {
        cmd.env(key, value);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn gemini: {e}"))?;

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
        let mut exit_code: Option<i32> = None;

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

        // Check exit status
        if let Ok(mut sessions) = ACTIVE_SESSIONS.lock() {
            if let Some(mut child) = sessions.remove(&sid) {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        exit_code = status.code();
                    }
                    _ => {}
                }
            }
        }

        // Map exit code to message
        if let Some(msg) = map_exit_code(exit_code) {
            on_error(msg);
        }
    });

    Ok(session_id)
}

/// Abort an active Gemini session.
pub fn abort_gemini_session(session_id: &str) -> Result<(), String> {
    let mut sessions = ACTIVE_SESSIONS.lock().map_err(|e| format!("Lock error: {e}"))?;
    if let Some(mut child) = sessions.remove(session_id) {
        let _ = child.start_kill();
        let _ = child.try_wait();
    }
    Ok(())
}

/// Check if a Gemini session is active.
pub fn is_gemini_session_active(session_id: &str) -> bool {
    ACTIVE_SESSIONS
        .lock()
        .map(|sessions| sessions.contains_key(session_id))
        .unwrap_or(false)
}

/// Get all active Gemini session IDs.
pub fn get_active_gemini_sessions() -> Vec<String> {
    ACTIVE_SESSIONS
        .lock()
        .map(|sessions| sessions.keys().cloned().collect())
        .unwrap_or_default()
}
