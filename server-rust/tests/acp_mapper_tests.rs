//! ACP mapper unit tests (no live agents required).

#![cfg(feature = "acp-bridge")]

use agent_client_protocol::schema::{
    ContentBlock, ContentChunk, SessionId, SessionNotification, SessionUpdate, TextContent,
    ToolCall, ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
};
use cloudcli_server::acp::mapper::ws_inbound::{
    build_content_blocks, parse_permission_response, parse_provider_command, ProviderCommand,
};
use cloudcli_server::acp::mapper::ws_outbound::{
    complete, map_session_notification, permission_cancelled, permission_request, session_created,
};
use cloudcli_server::shared::types::LlmProvider;
use std::path::PathBuf;

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

#[test]
fn thinking_chunk_maps_to_thinking_kind() {
    let notif = SessionNotification::new(
        SessionId::new("acp-1"),
        SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("hmm"),
        ))),
    );
    let json = map_session_notification(&notif, "ui-1", LlmProvider::Claude).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["kind"], "thinking");
    assert_eq!(v["content"], "hmm");
}

#[test]
fn tool_call_maps_to_tool_use() {
    let notif = SessionNotification::new(
        SessionId::new("acp-1"),
        SessionUpdate::ToolCall(
            ToolCall::new(ToolCallId::new("t1"), "Write")
                .raw_input(serde_json::json!({"path": "a.txt"})),
        ),
    );
    let json = map_session_notification(&notif, "ui-2", LlmProvider::Claude).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["kind"], "tool_use");
    assert_eq!(v["toolName"], "Write");
    assert_eq!(v["toolId"], "t1");
}

#[test]
fn tool_call_update_maps_to_tool_result() {
    let notif = SessionNotification::new(
        SessionId::new("acp-1"),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            ToolCallId::new("t1"),
            ToolCallUpdateFields::new().raw_output(serde_json::json!({"ok": true})),
        )),
    );
    let json = map_session_notification(&notif, "ui-2", LlmProvider::Claude).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["kind"], "tool_result");
    assert_eq!(v["toolId"], "t1");
}

#[test]
fn ws_outbound_session_created_and_complete_shapes() {
    let created: serde_json::Value =
        serde_json::from_str(&session_created("sid-1", LlmProvider::Claude)).unwrap();
    assert_eq!(created["kind"], "session_created");
    assert_eq!(created["newSessionId"], "sid-1");

    let done: serde_json::Value =
        serde_json::from_str(&complete("sid-1", LlmProvider::Claude)).unwrap();
    assert_eq!(done["kind"], "complete");
    assert_eq!(done["sessionId"], "sid-1");
}

#[test]
fn ws_outbound_permission_messages() {
    let req = permission_request(
        "sid-1",
        LlmProvider::Claude,
        "req-1",
        "Bash",
        serde_json::json!({"command": "ls"}),
    );
    let v: serde_json::Value = serde_json::from_str(&req).unwrap();
    assert_eq!(v["kind"], "permission_request");
    assert_eq!(v["requestId"], "req-1");

    let cancelled = permission_cancelled("sid-1", LlmProvider::Claude, "req-1", "user");
    let c: serde_json::Value = serde_json::from_str(&cancelled).unwrap();
    assert_eq!(c["kind"], "permission_cancelled");
}

#[test]
fn parse_claude_command_from_ws_payload() {
    let payload = serde_json::json!({
        "type": "claude-command",
        "command": "hello",
        "sessionId": "ws-sid-1",
        "options": {
            "cwd": "/tmp/project",
            "skipPermissions": false,
            "permissionMode": "plan"
        }
    });
    let cmd = parse_provider_command("claude-command", &payload).expect("parse");
    assert_eq!(cmd.provider, LlmProvider::Claude);
    assert_eq!(cmd.command, "hello");
    assert_eq!(cmd.ui_session_id, "ws-sid-1");
    assert_eq!(cmd.cwd, PathBuf::from("/tmp/project"));
    assert!(!cmd.skip_permissions);
    assert_eq!(cmd.permission_mode.as_deref(), Some("plan"));
}

#[test]
fn build_content_blocks_includes_text_and_images() {
    let cmd = ProviderCommand {
        provider: LlmProvider::Claude,
        command: "describe".into(),
        cwd: PathBuf::from("."),
        ui_session_id: "s".into(),
        resume: false,
        model: None,
        permission_mode: None,
        skip_permissions: true,
        images: vec![cloudcli_server::acp::mapper::ws_inbound::ImageAttachment {
            data: "abc".into(),
            mime_type: "image/png".into(),
        }],
        trust: false,
        sandbox_mode: None,
    };
    let blocks = build_content_blocks(&cmd);
    assert_eq!(blocks.len(), 2);
}

#[test]
fn parse_permission_response_extracts_decision() {
    let payload = serde_json::json!({
        "type": "claude-permission-response",
        "requestId": "req-42",
        "allow": true
    });
    let (id, decision) = parse_permission_response(&payload).expect("parse");
    assert_eq!(id, "req-42");
    assert!(decision.allow);
}
