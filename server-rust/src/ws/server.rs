//! WebSocket server — mirrors server/modules/websocket/services/websocket-server.service.ts
//!
//! Routes /ws (chat), /shell with auth verification and real PTY shell.

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

use crate::auth::middleware;

#[cfg(feature = "acp-bridge")]
use crate::acp::bridge::AcpBridge;
#[cfg(feature = "acp-bridge")]
use crate::acp::mapper::ws_inbound::{parse_provider_command, provider_from_msg_type};

pub fn ws_router() -> Router {
    #[cfg(feature = "acp-bridge")]
    let bridge = Arc::new(AcpBridge::new());
    Router::new()
        .route("/ws", get({
            #[cfg(feature = "acp-bridge")]
            let bridge = bridge.clone();
            #[cfg(feature = "acp-bridge")]
            {
                move |ws, query, req| ws_chat_handler(ws, query, req, bridge.clone())
            }
            #[cfg(not(feature = "acp-bridge"))]
            {
                |ws, query, req| ws_chat_handler(ws, query, req)
            }
        }))
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
    #[cfg(feature = "acp-bridge")] bridge: Arc<AcpBridge>,
) -> impl IntoResponse {
    let token = extract_token(&query, &req);
    let auth_user = middleware::authenticate_websocket(token.as_deref());
    #[cfg(feature = "acp-bridge")]
    {
        ws.on_upgrade(move |socket| handle_chat(socket, auth_user, bridge))
    }
    #[cfg(not(feature = "acp-bridge"))]
    {
        ws.on_upgrade(move |socket| handle_chat(socket, auth_user))
    }
}

#[cfg(feature = "acp-bridge")]
async fn handle_chat(
    socket: WebSocket,
    auth_user: Option<middleware::AuthUser>,
    bridge: Arc<AcpBridge>,
) {
    handle_chat_with_bridge(socket, auth_user, bridge).await;
}

#[cfg(not(feature = "acp-bridge"))]
async fn handle_chat(socket: WebSocket, auth_user: Option<middleware::AuthUser>) {
    handle_chat_legacy_only(socket, auth_user).await;
}

#[cfg(feature = "acp-bridge")]
async fn handle_chat_with_bridge(
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

                #[cfg(feature = "legacy-cli-agents")]
                if !crate::acp::acp_enabled() {
                    handle_legacy_message(msg_type, &parsed, session_id, direct_provider, &tx).await;
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

#[cfg(not(feature = "acp-bridge"))]
async fn handle_chat_legacy_only(
    socket: WebSocket,
    auth_user: Option<middleware::AuthUser>,
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

                handle_legacy_message(msg_type, &parsed, session_id, direct_provider, &tx).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    forward_task.abort();
    tracing::info!("WS chat closed: user={}", user.username);
}

#[cfg(feature = "acp-bridge")]
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

#[cfg(feature = "legacy-cli-agents")]
async fn handle_legacy_message(
    msg_type: &str,
    parsed: &serde_json::Value,
    session_id: Option<&str>,
    direct_provider: Option<&str>,
    tx: &tokio::sync::mpsc::Sender<String>,
) {
    let command = parsed.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let options = parsed.get("options");

    match msg_type {
        "claude-command" | "cursor-command" | "codex-command" | "gemini-command" => {
            let provider = match msg_type {
                "claude-command" => "claude",
                "cursor-command" => "cursor",
                "codex-command" => "codex",
                "gemini-command" | _ => "gemini",
            };

            let cwd = options
                .and_then(|o| o.get("cwd").or_else(|| o.get("projectPath")))
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let resume_flag = options
                .and_then(|o| o.get("resume"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let candidate_sid = session_id
                .or(options.and_then(|o| o.get("sessionId")).and_then(|v| v.as_str()));
            let resume_sid = if resume_flag && candidate_sid.is_some() {
                let sid = candidate_sid.unwrap();
                if is_agent_session_active(provider, sid) {
                    Some(sid)
                } else {
                    None
                }
            } else {
                None
            };
            let model = options.and_then(|o| o.get("model")).and_then(|v| v.as_str());
            let permission_mode = options.and_then(|o| o.get("permissionMode")).and_then(|v| v.as_str());
            let skip_permissions = options
                .and_then(|o| o.get("skipPermissions"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let tx_clone = tx.clone();
            let provider_owned = provider.to_string();
            let provider_owned2 = provider_owned.clone();
            let cmd_owned = command.to_string();
            let cwd_owned = cwd.to_string();
            let resume_owned = resume_sid.map(String::from);
            let model_owned = model.map(String::from);
            let pm_owned = permission_mode.map(String::from);

            let sid = candidate_sid
                .map(String::from)
                .or(resume_owned.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let created = serde_json::json!({
                "kind": "session_created",
                "newSessionId": sid,
                "sessionId": sid,
                "provider": provider_owned,
                "status": "started"
            });
            tx_clone.send(created.to_string()).await.ok();

            let (agent_tx, mut agent_rx) = tokio::sync::mpsc::channel::<String>(256);
            let sid_clone = sid.clone();
            tokio::spawn(async move {
                let _ = spawn_agent_with_channel(
                    &provider_owned,
                    &cmd_owned,
                    &cwd_owned,
                    resume_owned.as_deref(),
                    model_owned.as_deref(),
                    pm_owned.as_deref(),
                    skip_permissions,
                    &sid_clone,
                    agent_tx,
                )
                .await;
            });

            let tx_fwd = tx_clone.clone();
            let sid_fwd = sid.clone();
            tokio::spawn(async move {
                while let Some(line) = agent_rx.recv().await {
                    let msg = normalize_agent_line(&line, &sid_fwd, &provider_owned2);
                    if tx_fwd.send(msg).await.is_err() {
                        break;
                    }
                }
                let complete = serde_json::json!({
                    "kind": "complete",
                    "newSessionId": sid_fwd,
                    "sessionId": sid_fwd,
                    "provider": provider_owned2
                });
                tx_fwd.send(complete.to_string()).await.ok();
            });
        }
        "abort-session" | "abort" | "cancel" => {
            let sid = session_id.unwrap_or("");
            let prov = direct_provider.unwrap_or("claude");
            abort_agent(prov, sid);
            let response = serde_json::json!({
                "type": "session_aborted",
                "sessionId": sid,
                "status": "cancelled"
            });
            tx.send(response.to_string()).await.ok();
        }
        "check-session-status" => {
            let sid = session_id.unwrap_or("");
            let prov = direct_provider.unwrap_or("claude");
            let active = is_agent_session_active(prov, sid);
            let response = serde_json::json!({
                "type": "session-status",
                "sessionId": sid,
                "provider": prov,
                "isProcessing": active
            });
            tx.send(response.to_string()).await.ok();
        }
        "get-active-sessions" => {
            let sessions = get_active_sessions();
            let response = serde_json::json!({
                "type": "active-sessions",
                "sessions": sessions
            });
            tx.send(response.to_string()).await.ok();
        }
        _ => {
            let response = serde_json::json!({
                "type": "ack",
                "messageType": msg_type
            });
            tx.send(response.to_string()).await.ok();
        }
    }
}

#[cfg(feature = "legacy-cli-agents")]
async fn spawn_agent_with_channel(
    provider: &str,
    command: &str,
    cwd: &str,
    resume_session_id: Option<&str>,
    model: Option<&str>,
    permission_mode: Option<&str>,
    skip_permissions: bool,
    session_id: &str,
    agent_tx: tokio::sync::mpsc::Sender<String>,
) {
    let sid = session_id.to_string();
    let prov = provider.to_string();

    match provider {
        "claude" => {
            let tx = agent_tx.clone();
            let sid_c = sid.clone();
            let prov_c = prov.clone();
            let _ = crate::agents::claude_agent::spawn_claude(
                crate::agents::claude_agent::ClaudeOptions {
                    session_id: resume_session_id.map(String::from),
                    project_path: cwd.to_string(),
                    cwd: Some(cwd.to_string()),
                    command: Some(command.to_string()),
                    model: model.map(String::from),
                    permission_mode: permission_mode.map(String::from),
                    skip_permissions,
                    tools_settings: None,
                },
                move |line| {
                    if !line.trim().is_empty() {
                        let msg = normalize_agent_line(line, &sid_c, &prov_c);
                        let _ = tx.try_send(msg);
                    }
                },
                move |line| {
                    let err = format!(
                        r#"{{"kind":"error","sessionId":"{}","provider":"{}","content":{}}}"#,
                        sid,
                        prov,
                        serde_json::to_string(line).unwrap_or_else(|_| line.to_string())
                    );
                    let _ = agent_tx.try_send(err);
                },
            )
            .await;
        }
        "cursor" => {
            let tx = agent_tx.clone();
            let sid_c = sid.clone();
            let prov_c = prov.clone();
            let _ = crate::agents::cursor_agent::spawn_cursor(
                crate::agents::cursor_agent::CursorOptions {
                    session_id: resume_session_id.map(String::from),
                    project_path: cwd.to_string(),
                    cwd: Some(cwd.to_string()),
                    command: Some(command.to_string()),
                    model: model.map(String::from),
                    skip_permissions,
                    tools_settings: None,
                },
                move |line| {
                    if !line.trim().is_empty() {
                        let msg = normalize_agent_line(line, &sid_c, &prov_c);
                        let _ = tx.try_send(msg);
                    }
                },
                move |line| {
                    let err = format!(
                        r#"{{"kind":"error","sessionId":"{}","provider":"{}","content":{}}}"#,
                        sid,
                        prov,
                        serde_json::to_string(line).unwrap_or_else(|_| line.to_string())
                    );
                    let _ = agent_tx.try_send(err);
                },
            )
            .await;
        }
        "codex" => {
            let tx = agent_tx.clone();
            let sid_c = sid.clone();
            let prov_c = prov.clone();
            let _ = crate::agents::codex_agent::spawn_codex(
                crate::agents::codex_agent::CodexOptions {
                    session_id: resume_session_id.map(String::from),
                    project_path: cwd.to_string(),
                    cwd: Some(cwd.to_string()),
                    command: Some(command.to_string()),
                    model: model.map(String::from),
                    permission_mode: permission_mode.map(String::from),
                    sandbox_mode: None,
                },
                move |line| {
                    if !line.trim().is_empty() {
                        let msg = normalize_agent_line(line, &sid_c, &prov_c);
                        let _ = tx.try_send(msg);
                    }
                },
                move |line| {
                    let err = format!(
                        r#"{{"kind":"error","sessionId":"{}","provider":"{}","content":{}}}"#,
                        sid,
                        prov,
                        serde_json::to_string(line).unwrap_or_else(|_| line.to_string())
                    );
                    let _ = agent_tx.try_send(err);
                },
            )
            .await;
        }
        "gemini" => {
            let tx = agent_tx.clone();
            let sid_c = sid.clone();
            let prov_c = prov.clone();
            let _ = crate::agents::gemini_agent::spawn_gemini(
                crate::agents::gemini_agent::GeminiOptions {
                    session_id: resume_session_id.map(String::from),
                    project_path: cwd.to_string(),
                    cwd: Some(cwd.to_string()),
                    command: Some(command.to_string()),
                    model: model.map(String::from),
                    attach_images: None,
                    env_vars: std::collections::HashMap::new(),
                },
                move |line| {
                    if !line.trim().is_empty() {
                        let msg = normalize_agent_line(line, &sid_c, &prov_c);
                        let _ = tx.try_send(msg);
                    }
                },
                move |line| {
                    let err = format!(
                        r#"{{"kind":"error","sessionId":"{}","provider":"{}","content":{}}}"#,
                        sid,
                        prov,
                        serde_json::to_string(line).unwrap_or_else(|_| line.to_string())
                    );
                    let _ = agent_tx.try_send(err);
                },
            )
            .await;
        }
        _ => {
            let _ = agent_tx.try_send(format!(
                r#"{{"kind":"error","sessionId":"{}","provider":"{}","content":"Unknown provider"}}"#,
                session_id, provider
            ));
        }
    }
}

#[cfg(feature = "legacy-cli-agents")]
fn normalize_agent_line(line: &str, session_id: &str, provider: &str) -> String {
    let val: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            return serde_json::json!({
                "kind": "stream_delta",
                "sessionId": session_id,
                "provider": provider,
                "content": format!("{}\n", line)
            })
            .to_string();
        }
    };

    let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "system" => {
            let subtype = val.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
            if subtype == "init" {
                let agent_sid = val.get("session_id").and_then(|v| v.as_str()).unwrap_or(session_id);
                return serde_json::json!({
                    "kind": "session_created",
                    "newSessionId": agent_sid,
                    "sessionId": agent_sid,
                    "provider": provider,
                    "status": "started"
                })
                .to_string();
            }
            serde_json::json!({
                "kind": "status",
                "sessionId": session_id,
                "provider": provider,
                "content": val.to_string()
            })
            .to_string()
        }
        "assistant" => {
            let message = val.get("message");
            let content = message.and_then(|m| m.get("content"));
            let content_array = content.and_then(|c| c.as_array());

            if let Some(items) = content_array {
                for item in items {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if item_type == "tool_use" {
                        return serde_json::json!({
                            "kind": "tool_use",
                            "sessionId": session_id,
                            "provider": provider,
                            "content": {
                                "name": item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                                "id": item.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                                "input": item.get("input").unwrap_or(&serde_json::json!({}))
                            }
                        })
                        .to_string();
                    }
                }
                let combined: Vec<String> = items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|v| v.as_str()).map(String::from))
                    .collect();
                return serde_json::json!({
                    "kind": "text",
                    "sessionId": session_id,
                    "provider": provider,
                    "content": combined.join("")
                })
                .to_string();
            }

            serde_json::json!({
                "kind": "text",
                "sessionId": session_id,
                "provider": provider,
                "content": val.to_string()
            })
            .to_string()
        }
        "user" => {
            let message = val.get("message");
            let content = message.and_then(|m| m.get("content"));
            if let Some(items) = content.and_then(|c| c.as_array()) {
                for item in items {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if item_type == "tool_result" {
                        let result_content = item.get("content");
                        return serde_json::json!({
                            "kind": "tool_result",
                            "sessionId": session_id,
                            "provider": provider,
                            "content": result_content.unwrap_or(&serde_json::json!(""))
                        })
                        .to_string();
                    }
                }
            }
            serde_json::json!({
                "kind": "text",
                "sessionId": session_id,
                "provider": provider,
                "content": val.to_string()
            })
            .to_string()
        }
        "result" => {
            let subtype = val.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
            if subtype == "success" {
                return serde_json::json!({
                    "kind": "status",
                    "sessionId": session_id,
                    "provider": provider,
                    "content": "Completed successfully"
                })
                .to_string();
            }
            serde_json::json!({
                "kind": "error",
                "sessionId": session_id,
                "provider": provider,
                "content": val.to_string()
            })
            .to_string()
        }
        _ => serde_json::json!({
            "kind": "text",
            "sessionId": session_id,
            "provider": provider,
            "content": val.to_string()
        })
        .to_string(),
    }
}

#[cfg(feature = "legacy-cli-agents")]
fn abort_agent(provider: &str, session_id: &str) {
    match provider {
        "claude" => {
            let _ = crate::agents::claude_agent::abort_claude_session(session_id);
        }
        "cursor" => {
            let _ = crate::agents::cursor_agent::abort_cursor_session(session_id);
        }
        "codex" => {
            let _ = crate::agents::codex_agent::abort_codex_session(session_id);
        }
        "gemini" => {
            let _ = crate::agents::gemini_agent::abort_gemini_session(session_id);
        }
        _ => {}
    }
}

#[cfg(feature = "legacy-cli-agents")]
fn is_agent_session_active(provider: &str, session_id: &str) -> bool {
    match provider {
        "claude" => crate::agents::claude_agent::is_claude_session_active(session_id),
        "cursor" => crate::agents::cursor_agent::is_cursor_session_active(session_id),
        "codex" => crate::agents::codex_agent::is_codex_session_active(session_id),
        "gemini" => crate::agents::gemini_agent::is_gemini_session_active(session_id),
        _ => false,
    }
}

#[cfg(feature = "legacy-cli-agents")]
fn get_active_sessions() -> serde_json::Value {
    serde_json::json!({
        "claude": crate::agents::claude_agent::get_active_claude_sessions(),
        "codex": crate::agents::codex_agent::get_active_codex_sessions(),
        "cursor": crate::agents::cursor_agent::get_active_cursor_sessions(),
        "gemini": crate::agents::gemini_agent::get_active_gemini_sessions(),
    })
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
