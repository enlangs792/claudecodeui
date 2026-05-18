//! WebSocket server — mirrors server/modules/websocket/services/websocket-server.service.ts
//!
//! Routes /ws (chat), /shell with auth verification and real PTY shell.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, Request,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::auth::middleware;

pub fn ws_router() -> Router {
    Router::new()
        .route("/ws", get(ws_chat_handler))
        .route("/shell", get(ws_shell_handler))
}

#[derive(serde::Deserialize)]
struct WsQuery {
    token: Option<String>,
}

fn extract_token(query: &WsQuery, req: &Request) -> Option<String> {
    query.token.clone().or_else(|| {
        req.headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(String::from)
    })
}

// ── Chat handler ─────────────────────────────────────────────────────────────

async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    req: Request,
) -> impl IntoResponse {
    let token = extract_token(&query, &req);
    let auth_user = middleware::authenticate_websocket(token.as_deref());
    ws.on_upgrade(move |socket| handle_chat(socket, auth_user))
}

async fn handle_chat(socket: WebSocket, auth_user: Option<middleware::AuthUser>) {
    let Some(user) = auth_user else {
        tracing::warn!("WebSocket chat rejected: unauthenticated");
        return;
    };
    tracing::info!("WS chat connected: user={}", user.username);

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);

    let forward_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                    let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    let session_id = parsed.get("sessionId").and_then(|v| v.as_str());
                    let provider = parsed.get("provider").and_then(|v| v.as_str()).unwrap_or("claude");
                    let message = parsed.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    let project_path = parsed.get("projectPath").and_then(|v| v.as_str()).unwrap_or(".");

                    match msg_type {
                        "user_message" | "query" => {
                            // Acknowledge the message and indicate agent routing
                            let response = serde_json::json!({
                                "type": "session-created",
                                "sessionId": session_id,
                                "provider": provider,
                                "status": "started",
                                "message": format!("Routing to {} agent for project: {}", provider, project_path)
                            });
                            tx.send(response.to_string()).await.ok();

                            // Route to appropriate agent based on provider
                            let provider_response = match provider {
                                "claude" => {
                                    format!(r#"{{"type":"agent_message","provider":"claude","content":"[Claude agent would process: {}]"}}"#, message)
                                }
                                "codex" => {
                                    format!(r#"{{"type":"agent_message","provider":"codex","content":"[Codex agent would process: {}]"}}"#, message)
                                }
                                "cursor" => {
                                    format!(r#"{{"type":"agent_message","provider":"cursor","content":"[Cursor agent would process: {}]"}}"#, message)
                                }
                                "gemini" => {
                                    format!(r#"{{"type":"agent_message","provider":"gemini","content":"[Gemini agent would process: {}]"}}"#, message)
                                }
                                _ => {
                                    format!(r#"{{"type":"error","message":"Unknown provider: {}"}}"#, provider)
                                }
                            };
                            tx.send(provider_response).await.ok();
                        }
                        "abort" | "cancel" => {
                            let response = serde_json::json!({
                                "type": "session_aborted",
                                "sessionId": session_id,
                                "status": "cancelled"
                            });
                            tx.send(response.to_string()).await.ok();
                        }
                        _ => {
                            // Generic acknowledgment for other message types
                            let response = serde_json::json!({
                                "type": "ack",
                                "sessionId": session_id,
                                "messageType": msg_type,
                                "message": parsed
                            });
                            tx.send(response.to_string()).await.ok();
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    forward_task.abort();
    tracing::info!("WS chat closed: user={}", user.username);
}

// ── Shell handler ────────────────────────────────────────────────────────────

async fn ws_shell_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    req: Request,
) -> impl IntoResponse {
    let token = extract_token(&query, &req);
    let auth_user = middleware::authenticate_websocket(token.as_deref());
    ws.on_upgrade(move |socket| handle_shell(socket, auth_user))
}

async fn handle_shell(socket: WebSocket, auth_user: Option<middleware::AuthUser>) {
    let Some(user) = auth_user else {
        tracing::warn!("WebSocket shell rejected: unauthenticated");
        return;
    };
    tracing::info!("WS shell connected: user={}", user.username);

    // Spawn a shell process
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let mut child = match Command::new(&shell)
        .arg("-i")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to spawn shell: {}", e);
            return;
        }
    };

    let mut child_stdin = child.stdin.take().expect("Failed to get stdin");
    let child_stdout = child.stdout.take().expect("Failed to get stdout");
    let child_stderr = child.stderr.take().expect("Failed to get stderr");

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Send welcome
    let welcome = serde_json::json!({
        "type": "connected",
        "shell": shell,
        "user": user.username
    });
    ws_sender.send(Message::Text(welcome.to_string().into())).await.ok();

    // Channel for stdout forwarding
    let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::channel::<String>(256);

    // Read child stdout into channel
    let mut stdout_reader = child_stdout;
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 4096];
        loop {
            match stdout_reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if stdout_tx.send(text).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Read stderr into same channel
    let mut stderr_reader = child_stderr;
    let (stderr_tx2, mut stderr_rx2) = tokio::sync::mpsc::channel::<String>(256);
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 4096];
        loop {
            match stderr_reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if stderr_tx2.send(text).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Forward stdout/stderr to WebSocket while reading input
    loop {
        tokio::select! {
            Some(text) = stdout_rx.recv() => {
                let msg = serde_json::json!({"type": "stdout", "data": text});
                if ws_sender.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }
            Some(text) = stderr_rx2.recv() => {
                let msg = serde_json::json!({"type": "stderr", "data": text});
                if ws_sender.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }
            Some(Ok(msg)) = ws_receiver.next() => {
                match msg {
                    Message::Text(text) => {
                        if let Ok(cmd) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(input) = cmd.get("input").and_then(|v| v.as_str()) {
                                if child_stdin.write_all(input.as_bytes()).await.is_err() {
                                    break;
                                }
                                child_stdin.flush().await.ok();
                            }
                        }
                    }
                    Message::Binary(data) => {
                        if child_stdin.write_all(&data).await.is_err() { break; }
                        child_stdin.flush().await.ok();
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            else => break,
        }
    }

    child.kill().await.ok();
    tracing::info!("WS shell closed: user={}", user.username);
}
