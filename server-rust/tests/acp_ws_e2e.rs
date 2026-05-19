//! Cross-layer WebSocket integration: auth + `/ws` + mock ACP agent + claude-command.

#![cfg(feature = "acp-bridge")]

use std::time::Duration;

use axum::Router;
use cloudcli_server::auth::middleware;
use cloudcli_server::db::connection;
use cloudcli_server::db::migrations;
use cloudcli_server::db::repos::users::UserRepo;
use cloudcli_server::ws::server::ws_router;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn mock_agent_cmd() -> String {
    env!("CARGO_BIN_EXE_mock-acp-agent").to_string()
}

struct WsTestHarness {
    base_url: String,
    token: String,
    _server: tokio::task::JoinHandle<()>,
    _tempdir: tempfile::TempDir,
}

impl WsTestHarness {
    async fn new(scenario: Option<&str>) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let db_path = tempdir.path().join("test.sqlite");

        unsafe {
            std::env::set_var("JWT_SECRET", "ws-e2e-test-secret");
            std::env::set_var("DATABASE_URL", format!("sqlite://{}", db_path.display()));
            std::env::set_var("CLOUDCLI_ACP_CLAUDE_CMD", mock_agent_cmd());
            std::env::set_var("CLOUDCLI_ACP_BRIDGE", "1");
            std::env::remove_var("MOCK_ACP_SLOW");
            match scenario {
                Some(s) => std::env::set_var("MOCK_ACP_SCENARIO", s),
                None => std::env::remove_var("MOCK_ACP_SCENARIO"),
            }
        }

        connection::init_pool();
        migrations::initialize_database();
        let user = UserRepo::create_user("ws-test", "hash");
        let token = middleware::generate_token(user.id, &user.username);

        let app: Router = ws_router();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            base_url: format!("ws://{addr}/ws"),
            token,
            _server: server,
            _tempdir: tempdir,
        }
    }

    async fn connect(&self) -> tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    > {
        let url = format!("{}?token={}", self.base_url, self.token);
        let (ws, _) = connect_async(&url).await.expect("ws connect");
        ws
    }
}

fn parse_kind(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    v.get("kind").and_then(|k| k.as_str()).map(String::from)
}

async fn recv_json_until(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    stop_kind: &str,
    secs: u64,
) -> Vec<Value> {
    let mut out = Vec::new();
    let deadline = Duration::from_secs(secs);
    loop {
        match timeout(deadline, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                let done = v.get("kind").and_then(|k| k.as_str()) == Some(stop_kind);
                out.push(v);
                if done {
                    break;
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) => break,
            _ => {}
        }
    }
    out
}

async fn ws_claude_command_stream_complete() {
    let harness = WsTestHarness::new(None).await;
    let mut ws = harness.connect().await;

    let session_id = "ws-e2e-stream-1";
    let payload = serde_json::json!({
        "type": "claude-command",
        "command": "hello-ws",
        "sessionId": session_id,
        "options": {
            "cwd": env!("CARGO_MANIFEST_DIR"),
            "skipPermissions": true
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .expect("send command");

    let frames = recv_json_until(&mut ws, "complete", 30).await;
    let kinds: Vec<_> = frames
        .iter()
        .filter_map(|v| v.get("kind").and_then(|k| k.as_str()).map(String::from))
        .collect();

    assert!(
        kinds.iter().any(|k| k == "session_created"),
        "expected session_created, got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "stream_delta"),
        "expected stream_delta, got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "complete"),
        "expected complete, got {kinds:?}"
    );

    let combined: String = frames
        .iter()
        .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("stream_delta"))
        .filter_map(|v| v.get("content").and_then(|c| c.as_str()))
        .collect();
    assert!(
        combined.contains("echo: hello-ws"),
        "unexpected stream payload: {combined}"
    );

    ws.close(None).await.ok();
}

async fn ws_claude_command_tools() {
    let harness = WsTestHarness::new(Some("tools")).await;
    let mut ws = harness.connect().await;

    let payload = serde_json::json!({
        "type": "claude-command",
        "command": "tool-me",
        "sessionId": "ws-e2e-tools",
        "options": {
            "cwd": env!("CARGO_MANIFEST_DIR"),
            "skipPermissions": true
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .expect("send");

    let frames = recv_json_until(&mut ws, "complete", 30).await;
    let kinds: Vec<_> = frames
        .iter()
        .filter_map(|v| v.get("kind").and_then(|k| k.as_str()).map(String::from))
        .collect();

    assert!(kinds.iter().any(|k| k == "tool_use"), "expected tool_use");
    assert!(kinds.iter().any(|k| k == "tool_result"), "expected tool_result");
    assert!(kinds.iter().any(|k| k == "complete"), "expected complete");
    ws.close(None).await.ok();
}

async fn ws_rejects_unauthenticated() {
    let harness = WsTestHarness::new(None).await;
    let bad_url = format!("{}?token=not-a-valid-jwt", harness.base_url);
    let Ok((mut ws, _)) = connect_async(&bad_url).await else {
        return;
    };

    // Unauthenticated handler exits; socket may close without accepting commands.
    match timeout(Duration::from_secs(3), ws.next()).await {
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {}
        Ok(Some(Err(_))) => {}
        Ok(Some(Ok(Message::Text(text)))) => {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            assert!(
                v.get("kind").is_none() && v.get("type").and_then(|t| t.as_str()) != Some("session_created"),
                "unauthenticated socket should not stream chat frames: {text}"
            );
        }
        other => panic!("unexpected unauthenticated socket behavior: {other:?}"),
    }
    ws.close(None).await.ok();
}

async fn ws_abort_session() {
    let harness = WsTestHarness::new(Some("slow")).await;
    let mut ws = harness.connect().await;
    let session_id = "ws-e2e-abort";

    let payload = serde_json::json!({
        "type": "claude-command",
        "command": "slow",
        "sessionId": session_id,
        "options": {
            "cwd": env!("CARGO_MANIFEST_DIR"),
            "skipPermissions": true
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .expect("send");

    // Wait for session_created before abort
    let _ = recv_json_until(&mut ws, "session_created", 10).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    let abort = serde_json::json!({
        "type": "abort-session",
        "sessionId": session_id,
        "provider": "claude"
    });
    ws.send(Message::Text(abort.to_string().into()))
        .await
        .expect("abort");

    let status = serde_json::json!({
        "type": "check-session-status",
        "sessionId": session_id,
        "provider": "claude"
    });
    ws.send(Message::Text(status.to_string().into()))
        .await
        .expect("status");

    let mut saw_inactive = false;
    for _ in 0..20 {
        if let Ok(Some(Ok(Message::Text(text)))) =
            timeout(Duration::from_secs(2), ws.next()).await
        {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            if v.get("type").and_then(|t| t.as_str()) == Some("session-status")
                && v.get("isProcessing") == Some(&Value::Bool(false))
            {
                saw_inactive = true;
                break;
            }
        }
    }
    assert!(saw_inactive, "session should report not processing after abort");
    ws.close(None).await.ok();
}

async fn ws_claude_command_permission_flow() {
    let harness = WsTestHarness::new(Some("permission")).await;
    let mut ws = harness.connect().await;
    let session_id = "ws-e2e-perm";

    let payload = serde_json::json!({
        "type": "claude-command",
        "command": "perm",
        "sessionId": session_id,
        "options": {
            "cwd": env!("CARGO_MANIFEST_DIR"),
            "skipPermissions": false
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .expect("send command");

    let mut frames = recv_json_until(&mut ws, "permission_request", 30).await;
    let perm = frames
        .iter()
        .find(|v| v.get("kind").and_then(|k| k.as_str()) == Some("permission_request"))
        .expect("permission_request");
    let request_id = perm
        .get("requestId")
        .and_then(|v| v.as_str())
        .expect("requestId");

    let approve = serde_json::json!({
        "type": "claude-permission-response",
        "requestId": request_id,
        "allow": true,
        "sessionId": session_id
    });
    ws.send(Message::Text(approve.to_string().into()))
        .await
        .expect("approve");

    tokio::time::sleep(Duration::from_millis(300)).await;

    let pending_query = serde_json::json!({
        "type": "get-pending-permissions",
        "sessionId": session_id
    });
    ws.send(Message::Text(pending_query.to_string().into()))
        .await
        .expect("pending query");

    let mut pending_cleared = false;
    for _ in 0..10 {
        if let Ok(Some(Ok(Message::Text(text)))) =
            timeout(Duration::from_secs(2), ws.next()).await
        {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            if v.get("type").and_then(|t| t.as_str()) == Some("pending-permissions-response") {
                let data = v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
                pending_cleared = data.is_empty();
                break;
            }
        }
    }
    assert!(pending_cleared, "permission response should clear pending queue");

    ws.close(None).await.ok();
}

/// Serial WS E2E (env + DB); one harness per case.
#[tokio::test]
async fn acp_ws_e2e_suite() {
    ws_rejects_unauthenticated().await;
    ws_claude_command_stream_complete().await;
    ws_claude_command_tools().await;
    ws_claude_command_permission_flow().await;
    ws_abort_session().await;
}
