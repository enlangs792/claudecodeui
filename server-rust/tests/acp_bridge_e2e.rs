//! ACP bridge integration tests using `mock-acp-agent` (no external CLIs or API keys).

#![cfg(feature = "acp-bridge")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cloudcli_server::acp::bridge::AcpBridge;
use cloudcli_server::acp::mapper::ws_inbound::ProviderCommand;
use cloudcli_server::shared::types::LlmProvider;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::timeout;

fn mock_agent_cmd() -> String {
    env!("CARGO_BIN_EXE_mock-acp-agent").to_string()
}

fn install_mock_claude_cmd() {
    // SAFETY: tests set env before spawning agents; avoid cross-test leakage.
    unsafe {
        std::env::set_var("CLOUDCLI_ACP_CLAUDE_CMD", mock_agent_cmd());
        std::env::set_var("CLOUDCLI_ACP_BRIDGE", "1");
        std::env::remove_var("MOCK_ACP_SLOW");
    }
}

fn install_mock_claude_cmd_slow() {
    unsafe {
        std::env::set_var(
            "CLOUDCLI_ACP_CLAUDE_CMD",
            format!("env MOCK_ACP_SLOW=1 {}", mock_agent_cmd()),
        );
        std::env::set_var("CLOUDCLI_ACP_BRIDGE", "1");
    }
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

fn parse_kind(msg: &str) -> Option<String> {
    let v: Value = serde_json::from_str(msg).ok()?;
    v.get("kind").and_then(|k| k.as_str()).map(String::from)
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

async fn mock_agent_direct_acp_roundtrip_inner() {
    use agent_client_protocol::schema::{
        ContentBlock, InitializeRequest, PromptRequest, ProtocolVersion, TextContent,
    };
    use agent_client_protocol::{Agent, Client, ConnectionTo};
    use std::str::FromStr;

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
        kinds.iter().any(|k| k == "complete"),
        "expected complete, got kinds: {kinds:?}"
    );

    let stream = messages
        .iter()
        .find(|m| parse_kind(m).as_deref() == Some("stream_delta"))
        .expect("stream_delta message");
    let v: Value = serde_json::from_str(stream).unwrap();
    assert_eq!(v["sessionId"], session_id);
    assert!(
        v["content"]
            .as_str()
            .unwrap_or("")
            .contains("echo: ping"),
        "unexpected stream content: {}",
        v["content"]
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

    // Wait until session is active
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

    // Drain messages until idle or timeout
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
    bridge_abort_active_session_inner().await;
}
