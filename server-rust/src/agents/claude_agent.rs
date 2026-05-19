//! Claude Agent — mirrors server/claude-sdk.js
//!
//! Spawns the `claude` CLI as a subprocess with --output-format stream-json.
//! Provides session management (spawn, abort, status).

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};

/// Active Claude sessions (session_id -> Child process handle)
static ACTIVE_SESSIONS: LazyLock<Mutex<HashMap<String, ActiveSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct ActiveSession {
    child: Child,
}

pub struct ClaudeOptions {
    pub session_id: Option<String>,
    pub project_path: String,
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub skip_permissions: bool,
    pub tools_settings: Option<ToolsSettings>,
}

pub struct ToolsSettings {
    pub allowed_shell_commands: Vec<String>,
    pub skip_permissions: bool,
}

impl Default for ToolsSettings {
    fn default() -> Self {
        ToolsSettings {
            allowed_shell_commands: vec![],
            skip_permissions: false,
        }
    }
}

/// Spawn a Claude CLI process and stream its output via a callback.
/// Returns the session_id.
pub async fn spawn_claude(
    options: ClaudeOptions,
    on_message: impl Fn(&str) + Send + Sync + 'static,
    on_error: impl Fn(&str) + Send + Sync + 'static,
) -> Result<String, String> {
    let session_id = options
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut args: Vec<String> = Vec::new();

    // Resume existing session or start new
    if options.session_id.is_some() {
        args.push("--resume".into());
        args.push(session_id.clone());
    }

    // Add prompt if provided
    if let Some(ref cmd) = options.command {
        if !cmd.trim().is_empty() {
            args.push("-p".into());
            args.push(cmd.clone());
        }
    }

    // Model selection
    if let Some(ref model) = options.model {
        args.push("--model".into());
        args.push(model.clone());
    }

    // Permission mode — default to bypass (matching TS behavior)
    if options.skip_permissions {
        args.push("-f".into());
    } else if let Some(ref mode) = options.permission_mode {
        if mode == "bypassPermissions" || mode == "default" {
            // "default" in frontend maps to -f for CLI (no interactive approval in Rust)
            args.push("-f".into());
        } else {
            args.push("--permission-mode".into());
            args.push(mode.clone());
        }
    } else {
        // No permission mode specified — default to skip for non-interactive mode
        args.push("-f".into());
    }

    // Streaming JSON output (requires --verbose in recent Claude Code versions)
    args.push("--output-format".into());
    args.push("stream-json".into());
    args.push("--verbose".into());

    // Working directory
    let working_dir = options.cwd.unwrap_or_else(|| options.project_path.clone());

    let mut child = Command::new("claude")
        .args(&args)
        .current_dir(&working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn claude: {e}"))?;

    let stdout = child.stdout.take().ok_or("No stdout")?;
    let stderr = child.stderr.take().ok_or("No stderr")?;

    // Store session
    {
        let mut sessions = ACTIVE_SESSIONS.lock().map_err(|e| format!("Lock error: {e}"))?;
        sessions.insert(
            session_id.clone(),
            ActiveSession {
                child,
            },
        );
    }

    // Read stdout line by line
    let stdout_reader = tokio::io::BufReader::new(stdout);
    let mut stdout_lines = stdout_reader.lines();

    // Read stderr line by line
    let stderr_reader = tokio::io::BufReader::new(stderr);
    let mut stderr_lines = stderr_reader.lines();

    // Process stdout and stderr concurrently
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
                        Ok(None) => break, // stdout closed
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

        // Clean up session on completion
        if let Ok(mut sessions) = ACTIVE_SESSIONS.lock() {
            sessions.remove(&sid);
        }
    });

    Ok(session_id)
}

/// Abort an active Claude session by killing its process.
pub fn abort_claude_session(session_id: &str) -> Result<(), String> {
    let mut sessions = ACTIVE_SESSIONS.lock().map_err(|e| format!("Lock error: {e}"))?;
    if let Some(mut session) = sessions.remove(session_id) {
        if let Err(e) = session.child.start_kill() {
            return Err(format!("Failed to kill claude process: {e}"));
        }
        // Wait briefly for process to die
        let _ = session.child.try_wait();
    }
    Ok(())
}

/// Check if a Claude session is currently active.
pub fn is_claude_session_active(session_id: &str) -> bool {
    ACTIVE_SESSIONS
        .lock()
        .map(|sessions| sessions.contains_key(session_id))
        .unwrap_or(false)
}

/// Get all active Claude session IDs.
pub fn get_active_claude_sessions() -> Vec<String> {
    ACTIVE_SESSIONS
        .lock()
        .map(|sessions| sessions.keys().cloned().collect())
        .unwrap_or_default()
}

/// Write to a Claude session's stdin (async).
#[allow(dead_code)]
pub async fn write_to_claude_stdin(session_id: &str, data: &str) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut sessions = ACTIVE_SESSIONS.lock().map_err(|e| format!("Lock error: {e}"))?;
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| format!("Session not found: {session_id}"))?;
    if let Some(ref mut stdin) = session.child.stdin {
        stdin
            .write_all(data.as_bytes())
            .await
            .map_err(|e| format!("Write error: {e}"))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Write error: {e}"))?;
    }
    Ok(())
}
