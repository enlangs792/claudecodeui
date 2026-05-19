//! WebSocket server — mirrors server/modules/websocket/services/websocket-server.service.ts
//!
//! Routes /ws (chat via ACP bridge), /shell with auth verification and real PTY shell.

#[cfg(not(feature = "acp-bridge"))]
compile_error!("cloudcli-server requires the `acp-bridge` feature (legacy CLI agents were removed)");

use std::sync::Arc;

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

use crate::acp::bridge::AcpBridge;
use crate::acp::mapper::ws_inbound::{parse_provider_command, provider_from_msg_type};
use crate::auth::middleware;

pub fn ws_router() -> Router {
    let bridge = Arc::new(AcpBridge::new());
    Router::new()
        .route(
            "/ws",
            get({
                let bridge = bridge.clone();
                move |ws, query, req| ws_chat_handler(ws, query, req, bridge.clone())
            }),
        )
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
    bridge: Arc<AcpBridge>,
) -> impl IntoResponse {
    let token = extract_token(&query, &req);
    let auth_user = middleware::authenticate_websocket(token.as_deref());
    ws.on_upgrade(move |socket| handle_chat(socket, auth_user, bridge))
}

async fn handle_chat(
    socket: WebSocket,
    auth_user: Option<middleware::AuthUser>,
    bridge: Arc<AcpBridge>,
) {
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
                let parsed: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let msg_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let session_id = parsed.get("sessionId").and_then(|v| v.as_str());
                let direct_provider = parsed.get("provider").and_then(|v| v.as_str());

                if handle_acp_message(
                    msg_type,
                    &parsed,
                    session_id,
                    direct_provider,
                    &bridge,
                    &tx,
                )
                .await
                {
                    continue;
                }

                let response = serde_json::json!({
                    "type": "ack",
                    "messageType": msg_type
                });
                tx.send(response.to_string()).await.ok();
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    forward_task.abort();
    tracing::info!("WS chat closed: user={}", user.username);
}

async fn handle_acp_message(
    msg_type: &str,
    parsed: &serde_json::Value,
    session_id: Option<&str>,
    direct_provider: Option<&str>,
    bridge: &Arc<AcpBridge>,
    tx: &tokio::sync::mpsc::Sender<String>,
) -> bool {
    if !crate::acp::acp_enabled() {
        return false;
    }

    if let Some(cmd) = parse_provider_command(msg_type, parsed) {
        let bridge = Arc::clone(bridge);
        let tx = tx.clone();
        tokio::spawn(async move {
            bridge.handle_provider_command(cmd, tx).await;
        });
        return true;
    }

    if msg_type == "cursor-resume" {
        let cwd = parsed
            .get("options")
            .and_then(|o| o.get("cwd"))
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let sid = session_id.unwrap_or("");
        let mut resume_msg = parsed.clone();
        resume_msg["type"] = serde_json::json!("cursor-command");
        resume_msg["command"] = serde_json::json!("");
        resume_msg["options"] = serde_json::json!({
            "cwd": cwd,
            "resume": true,
            "sessionId": sid,
        });
        resume_msg["sessionId"] = serde_json::json!(sid);
        if let Some(cmd) = parse_provider_command("cursor-command", &resume_msg) {
            let bridge = Arc::clone(bridge);
            let tx = tx.clone();
            tokio::spawn(async move {
                bridge.handle_provider_command(cmd, tx).await;
            });
        }
        return true;
    }

    match msg_type {
        "claude-permission-response" => {
            crate::acp::bridge::handle_ws_permission_response(bridge, parsed).await;
            return true;
        }
        "get-pending-permissions" => {
            let sid = session_id.unwrap_or("");
            let pending = bridge.get_pending_permissions(sid).await;
            let response = serde_json::json!({
                "type": "pending-permissions-response",
                "sessionId": sid,
                "data": pending
            });
            tx.send(response.to_string()).await.ok();
            return true;
        }
        "abort-session" | "abort" | "cancel" => {
            let sid = session_id.unwrap_or("");
            let prov = direct_provider.unwrap_or("claude");
            bridge.abort_session(prov, sid);
            let response = serde_json::json!({
                "type": "session_aborted",
                "sessionId": sid,
                "status": "cancelled"
            });
            tx.send(response.to_string()).await.ok();
            return true;
        }
        "cursor-abort" => {
            let sid = session_id.unwrap_or("");
            let success = bridge.abort_session("cursor", sid);
            let response = serde_json::json!({
                "kind": "complete",
                "exitCode": if success { 0 } else { 1 },
                "aborted": true,
                "success": success,
                "sessionId": sid,
                "provider": "cursor"
            });
            tx.send(response.to_string()).await.ok();
            return true;
        }
        "check-session-status" => {
            let sid = session_id.unwrap_or("");
            let prov = direct_provider.unwrap_or("claude");
            let active = bridge.is_active(prov, sid);
            if active {
                bridge.reconnect_writer(prov, sid, tx.clone());
            }
            let response = serde_json::json!({
                "type": "session-status",
                "sessionId": sid,
                "provider": prov,
                "isProcessing": active
            });
            tx.send(response.to_string()).await.ok();
            return true;
        }
        "get-active-sessions" => {
            let sessions = bridge.get_active_sessions();
            let response = serde_json::json!({
                "type": "active-sessions",
                "sessions": sessions
            });
            tx.send(response.to_string()).await.ok();
            return true;
        }
        _ => {}
    }

    let _ = provider_from_msg_type(msg_type);
    false
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

    let welcome = serde_json::json!({
        "type": "connected",
        "shell": shell,
        "user": user.username
    });
    ws_sender
        .send(Message::Text(welcome.to_string().into()))
        .await
        .ok();

    let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::channel::<String>(256);

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
