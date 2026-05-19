//! ACP bridge integration tests using `mock-acp-agent` (no external CLIs or API keys).

#![cfg(feature = "acp-bridge")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cloudcli_server::acp::bridge::AcpBridge;
use cloudcli_server::acp::mapper::ws_inbound::ProviderCommand;
use cloudcli_server::acp::permissions::PermissionDecision;
use cloudcli_server::shared::types::LlmProvider;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::timeout;

fn mock_agent_cmd() -> String {
    env!("CARGO_BIN_EXE_mock-acp-agent").to_string()
}

fn install_mock_env(scenario: Option<&str>) {
    // SAFETY: serial test suite; env is set before spawning agents.
    unsafe {
        std::env::set_var("CLOUDCLI_ACP_CLAUDE_CMD", mock_agent_cmd());
        std::env::set_var("CLOUDCLI_ACP_BRIDGE", "1");
        std::env::remove_var("MOCK_ACP_SLOW");
        match scenario {
            Some(s) => std::env::set_var("MOCK_ACP_SCENARIO", s),
            None => std::env::remove_var("MOCK_ACP_SCENARIO"),
        }
    }
}

fn install_mock_claude_cmd() {
    install_mock_env(None);
}

fn install_mock_claude_cmd_slow() {
    unsafe {
        std::env::set_var("CLOUDCLI_ACP_CLAUDE_CMD", mock_agent_cmd());
        std::env::set_var("CLOUDCLI_ACP_BRIDGE", "1");
        std::env::set_var("MOCK_ACP_SCENARIO", "slow");
        std::env::remove_var("MOCK_ACP_SLOW");
    }
}

fn install_mock_scenario(scenario: &str) {
    install_mock_env(Some(scenario));
}

fn claude_command(ui_session_id: &str, text: &str) -> ProviderCommand {
    ProviderCommand {
        provider: LlmProvider::Claude,
        command: text.to_string(),
        cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ui_session_id: ui_session_id.to_string(),
        resume: false,
        model: None,
        permission_mode: None,
        skip_permissions: true,
        images: vec![],
        trust: false,
        sandbox_mode: None,
    }
}

fn claude_command_with_permissions(ui_session_id: &str, text: &str) -> ProviderCommand {
    let mut cmd = claude_command(ui_session_id, text);
    cmd.skip_permissions = false;
    cmd
}

fn parse_kind(msg: &str) -> Option<String> {
    let v: Value = serde_json::from_str(msg).ok()?;
    v.get("kind").and_then(|k| k.as_str()).map(String::from)
}

fn parse_json(msg: &str) -> Value {
    serde_json::from_str(msg).unwrap_or(Value::Null)
}

async fn collect_until_complete(
    rx: &mut mpsc::Receiver<String>,
    timeout_secs: u64,
) -> Vec<String> {
    let mut messages = Vec::new();
    let deadline = Duration::from_secs(timeout_secs);
    loop {
        match timeout(deadline, rx.recv()).await {
            Ok(Some(msg)) => {
                let done = parse_kind(&msg).as_deref() == Some("complete");
                messages.push(msg);
                if done {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    messages
}

async fn collect_until_kind(
    rx: &mut mpsc::Receiver<String>,
    kind: &str,
    timeout_secs: u64,
) -> Vec<String> {
    let mut messages = Vec::new();
    let deadline = Duration::from_secs(timeout_secs);
    loop {
        match timeout(deadline, rx.recv()).await {
            Ok(Some(msg)) => {
                let matched = parse_kind(&msg).as_deref() == Some(kind);
                messages.push(msg);
                if matched {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    messages
}

async fn mock_agent_direct_acp_roundtrip_inner() {
    use agent_client_protocol::schema::{
        ContentBlock, InitializeRequest, PromptRequest, ProtocolVersion, TextContent,
    };
    use agent_client_protocol::{Agent, Client, ConnectionTo};
    use std::str::FromStr;

    install_mock_claude_cmd();

    let agent = agent_client_protocol::AcpAgent::from_str(&mock_agent_cmd())
        .expect("spawn mock agent");

    let got_chunk = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let got_chunk_cb = got_chunk.clone();

    Client
        .builder()
        .on_receive_notification(
            {
                async move |notif: agent_client_protocol::schema::SessionNotification, _| {
                    if matches!(
                        notif.update,
                        agent_client_protocol::schema::SessionUpdate::AgentMessageChunk(_)
                    ) {
                        got_chunk_cb.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;

            let mut session = connection
                .build_session(env!("CARGO_MANIFEST_DIR"))
                .block_task()
                .start_session()
                .await?;

            session
                .connection()
                .send_request_to(
                    Agent,
                    PromptRequest::new(
                        session.session_id().clone(),
                        vec![ContentBlock::Text(TextContent::new("hi"))],
                    ),
                )
                .block_task()
                .await?;

            Ok(())
        })
        .await
        .expect("mock agent roundtrip");

    assert!(
        got_chunk.load(std::sync::atomic::Ordering::SeqCst),
        "mock agent should emit AgentMessageChunk notification"
    );
}

async fn bridge_mock_agent_stream_and_complete_inner() {
    install_mock_claude_cmd();
    let bridge = AcpBridge::new();
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let session_id = "e2e-mock-session-1".to_string();

    bridge
        .handle_provider_command(claude_command(&session_id, "ping"), tx)
        .await;

    let messages = collect_until_complete(&mut rx, 30).await;
    let kinds: Vec<_> = messages.iter().filter_map(|m| parse_kind(m)).collect();

    assert!(
        kinds.iter().any(|k| k == "session_created"),
        "expected session_created, got kinds: {kinds:?} messages: {messages:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "stream_delta"),
        "expected stream_delta, got kinds: {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "thinking"),
        "expected thinking chunk in stream scenario, got kinds: {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "complete"),
        "expected complete, got kinds: {kinds:?}"
    );

    let stream_deltas: Vec<_> = messages
        .iter()
        .filter(|m| parse_kind(m).as_deref() == Some("stream_delta"))
        .collect();
    assert!(
        stream_deltas.len() >= 2,
        "stream scenario should emit multiple deltas, got {}",
        stream_deltas.len()
    );

    let combined: String = stream_deltas
        .iter()
        .map(|m| parse_json(m)["content"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        combined.contains("echo: ping"),
        "unexpected stream content: {combined}"
    );
}

async fn bridge_mock_agent_tools_inner() {
    install_mock_scenario("tools");
    let bridge = AcpBridge::new();
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let session_id = "e2e-tools-session".to_string();

    bridge
        .handle_provider_command(claude_command(&session_id, "run-tool"), tx)
        .await;

    let messages = collect_until_complete(&mut rx, 30).await;
    let kinds: Vec<_> = messages.iter().filter_map(|m| parse_kind(m)).collect();

    assert!(kinds.iter().any(|k| k == "tool_use"), "expected tool_use, got {kinds:?}");
    assert!(
        kinds.iter().any(|k| k == "tool_result"),
        "expected tool_result, got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "stream_delta"),
        "expected stream_delta after tools, got {kinds:?}"
    );
    assert!(kinds.iter().any(|k| k == "complete"), "expected complete");

    let tool_use = messages
        .iter()
        .find(|m| parse_kind(m).as_deref() == Some("tool_use"))
        .expect("tool_use message");
    let v = parse_json(tool_use);
    assert_eq!(v["toolName"], "Read");
    assert_eq!(v["toolId"], "mock-tool-1");
}

async fn bridge_permission_request_and_resolve_inner() {
    install_mock_scenario("permission");
    let bridge = Arc::new(AcpBridge::new());
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let session_id = "e2e-perm-session".to_string();

    let bridge_bg = Arc::clone(&bridge);
    let cmd = claude_command_with_permissions(&session_id, "need-perm");
    tokio::spawn(async move {
        bridge_bg.handle_provider_command(cmd, tx).await;
    });

    let messages = collect_until_kind(&mut rx, "permission_request", 30).await;
    let perm_msg = messages
        .iter()
        .find(|m| parse_kind(m).as_deref() == Some("permission_request"))
        .expect("permission_request frame");
    let request_id = parse_json(perm_msg)["requestId"]
        .as_str()
        .expect("requestId")
        .to_string();

    assert_eq!(parse_json(perm_msg)["toolName"], "Bash");

    assert!(
        bridge
            .handle_permission_response(
                &request_id,
                PermissionDecision {
                    allow: true,
                    updated_input: None,
                    message: None,
                },
            )
            .await,
        "permission should resolve"
    );
}

async fn bridge_mock_agent_error_inner() {
    install_mock_scenario("error");
    let bridge = AcpBridge::new();
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let session_id = "e2e-error-session".to_string();

    bridge
        .handle_provider_command(claude_command(&session_id, "fail"), tx)
        .await;

    let messages = collect_until_complete(&mut rx, 30).await;
    let kinds: Vec<_> = messages.iter().filter_map(|m| parse_kind(m)).collect();

    assert!(
        kinds.iter().any(|k| k == "error") || kinds.iter().any(|k| k == "complete"),
        "error scenario should surface error or complete, got {kinds:?}"
    );
    assert!(
        !bridge.is_active("claude", &session_id),
        "session should not remain active after error"
    );
}

async fn bridge_abort_active_session_inner() {
    install_mock_claude_cmd_slow();

    let bridge = Arc::new(AcpBridge::new());
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let session_id = "e2e-abort-session".to_string();

    let bridge_bg = Arc::clone(&bridge);
    let cmd = claude_command(&session_id, "slow");
    tokio::spawn(async move {
        bridge_bg.handle_provider_command(cmd, tx).await;
    });

    let mut started = false;
    for _ in 0..50 {
        if bridge.as_ref().is_active("claude", &session_id) {
            started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(started, "session never became active");

    assert!(bridge.as_ref().abort_session("claude", &session_id));

    let _ = collect_until_complete(&mut rx, 10).await;

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !bridge.as_ref().is_active("claude", &session_id),
        "session should not be active after abort"
    );
}

/// Serial ACP bridge E2E (mutates process env; do not run cases in parallel).
#[tokio::test]
async fn acp_bridge_e2e_suite() {
    mock_agent_direct_acp_roundtrip_inner().await;
    bridge_mock_agent_stream_and_complete_inner().await;
    bridge_mock_agent_tools_inner().await;
    bridge_permission_request_and_resolve_inner().await;
    bridge_mock_agent_error_inner().await;
    bridge_abort_active_session_inner().await;
}
