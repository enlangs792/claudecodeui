//! WebSocket server — mirrors server/modules/websocket/services/websocket-server.service.ts
//!
//! Routes /ws (chat), /shell, and /plugin-ws paths with auth verification.

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
use std::sync::Arc;

use crate::auth::middleware;

/// Create the WebSocket router
pub fn ws_router() -> Router {
    Router::new()
        .route("/ws", get(ws_chat_handler))
        .route("/shell", get(ws_shell_handler))
}

/// Query params for WebSocket connections
#[derive(serde::Deserialize)]
struct WsQuery {
    token: Option<String>,
}

/// GET /ws — Chat WebSocket upgrade
async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    req: Request,
) -> impl IntoResponse {
    let token = query.token.or_else(|| {
        // Extract from Authorization header
        req.headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(String::from)
    });

    // Authenticate
    let auth_user = middleware::authenticate_websocket(token.as_deref());

    ws.on_upgrade(move |socket| handle_chat(socket, auth_user))
}

/// GET /shell — Shell WebSocket upgrade
async fn ws_shell_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    req: Request,
) -> impl IntoResponse {
    let token = query.token.or_else(|| {
        req.headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(String::from)
    });

    let auth_user = middleware::authenticate_websocket(token.as_deref());

    ws.on_upgrade(move |socket| handle_shell(socket, auth_user))
}

/// Handle an authenticated chat WebSocket connection
async fn handle_chat(socket: WebSocket, auth_user: Option<middleware::AuthUser>) {
    if auth_user.is_none() {
        tracing::warn!("WebSocket chat connection rejected: unauthenticated");
        return;
    }

    let user = auth_user.unwrap();
    tracing::info!("WebSocket chat connected: user={}", user.username);

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);

    // Spawn a task to forward channel messages to the WebSocket
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Read loop — handle incoming messages
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                tracing::debug!("Chat message from {}: {} chars", user.username, text.len());
                // Parse and handle the message; echo for now
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                    let response = serde_json::json!({
                        "type": "ack",
                        "message": parsed
                    });
                    tx.send(response.to_string()).await.ok();
                }
            }
            Message::Close(_) => {
                tracing::info!("WebSocket chat closed: user={}", user.username);
                break;
            }
            _ => {}
        }
    }

    forward_task.abort();
}

/// Handle an authenticated shell WebSocket connection
async fn handle_shell(socket: WebSocket, auth_user: Option<middleware::AuthUser>) {
    if auth_user.is_none() {
        tracing::warn!("WebSocket shell connection rejected: unauthenticated");
        return;
    }

    let user = auth_user.unwrap();
    tracing::info!("WebSocket shell connected: user={}", user.username);

    let (mut sender, mut receiver) = socket.split();

    // Send a welcome message
    let welcome = serde_json::json!({
        "type": "connected",
        "message": "Shell session started"
    });
    sender
        .send(Message::Text(welcome.to_string().into()))
        .await
        .ok();

    // Read loop
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                tracing::debug!("Shell input: {} bytes", text.len());
            }
            Message::Binary(data) => {
                tracing::debug!("Shell binary: {} bytes", data.len());
            }
            Message::Close(_) => {
                tracing::info!("WebSocket shell closed: user={}", user.username);
                break;
            }
            _ => {}
        }
    }
}
