//! ACP mapper unit tests (no live agents required).

#![cfg(feature = "acp-bridge")]

use agent_client_protocol::schema::{
    ContentBlock, ContentChunk, SessionId, SessionNotification, SessionUpdate, TextContent,
};
use cloudcli_server::acp::mapper::ws_outbound::map_session_notification;
use cloudcli_server::shared::types::LlmProvider;

#[test]
fn stream_delta_from_agent_chunk() {
    let notif = SessionNotification::new(
        SessionId::new("acp-1"),
        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("hi"),
        ))),
    );
    let json = map_session_notification(&notif, "ui-1", LlmProvider::Claude).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["kind"], "stream_delta");
    assert_eq!(v["sessionId"], "ui-1");
    assert_eq!(v["content"], "hi");
}
