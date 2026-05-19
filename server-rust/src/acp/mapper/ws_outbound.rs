//! Map ACP session notifications to frontend WS JSON (kind field).

use agent_client_protocol::schema::{
    ContentBlock, ContentChunk, SessionNotification, SessionUpdate, ToolCall, ToolCallUpdate,
};
use serde_json::json;

use crate::shared::types::LlmProvider;

pub fn map_session_notification(
    notification: &SessionNotification,
    ui_session_id: &str,
    provider: LlmProvider,
) -> Option<String> {
    let update = &notification.update;
    let sid = ui_session_id;
    let prov = provider.as_str();

    let msg = match update {
        SessionUpdate::AgentMessageChunk(ContentChunk {
            content: ContentBlock::Text(text),
            ..
        }) => json!({
            "kind": "stream_delta",
            "sessionId": sid,
            "provider": prov,
            "content": text.text
        }),
        SessionUpdate::AgentMessageChunk(ContentChunk { content, .. }) => json!({
            "kind": "text",
            "sessionId": sid,
            "provider": prov,
            "content": format!("{:?}", content)
        }),
        SessionUpdate::AgentThoughtChunk(ContentChunk {
            content: ContentBlock::Text(text),
            ..
        }) => json!({
            "kind": "thinking",
            "sessionId": sid,
            "provider": prov,
            "content": text.text
        }),
        SessionUpdate::ToolCall(ToolCall {
            tool_call_id,
            title,
            raw_input,
            ..
        }) => json!({
            "kind": "tool_use",
            "sessionId": sid,
            "provider": prov,
            "toolName": title,
            "toolId": tool_call_id.to_string(),
            "toolInput": raw_input.clone().unwrap_or(json!({}))
        }),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate {
            tool_call_id,
            fields,
            ..
        }) => {
            let content = fields
                .raw_output
                .clone()
                .or(fields.raw_input.clone())
                .unwrap_or(json!({}));
            json!({
                "kind": "tool_result",
                "sessionId": sid,
                "provider": prov,
                "toolId": tool_call_id.to_string(),
                "content": content
            })
        }
        SessionUpdate::UserMessageChunk(_) => return None,
        _ => json!({
            "kind": "status",
            "sessionId": sid,
            "provider": prov,
            "content": format!("{:?}", update)
        }),
    };

    Some(msg.to_string())
}

pub fn session_created(ui_session_id: &str, provider: LlmProvider) -> String {
    json!({
        "kind": "session_created",
        "newSessionId": ui_session_id,
        "sessionId": ui_session_id,
        "provider": provider.as_str(),
        "status": "started"
    })
    .to_string()
}

pub fn complete(ui_session_id: &str, provider: LlmProvider) -> String {
    json!({
        "kind": "complete",
        "newSessionId": ui_session_id,
        "sessionId": ui_session_id,
        "provider": provider.as_str()
    })
    .to_string()
}

pub fn error_message(ui_session_id: &str, provider: LlmProvider, content: &str) -> String {
    json!({
        "kind": "error",
        "sessionId": ui_session_id,
        "provider": provider.as_str(),
        "content": content
    })
    .to_string()
}

pub fn permission_request(
    ui_session_id: &str,
    provider: LlmProvider,
    request_id: &str,
    tool_name: &str,
    input: serde_json::Value,
) -> String {
    json!({
        "kind": "permission_request",
        "requestId": request_id,
        "toolName": tool_name,
        "input": input,
        "sessionId": ui_session_id,
        "provider": provider.as_str()
    })
    .to_string()
}

pub fn permission_cancelled(
    ui_session_id: &str,
    provider: LlmProvider,
    request_id: &str,
    reason: &str,
) -> String {
    json!({
        "kind": "permission_cancelled",
        "requestId": request_id,
        "reason": reason,
        "sessionId": ui_session_id,
        "provider": provider.as_str()
    })
    .to_string()
}

pub fn map_gemini_exit_code(stderr: &str) -> Option<String> {
    if stderr.contains("trust") || stderr.contains("workspace") {
        return Some("Cursor workspace trust required".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::{SessionId, TextContent};

    #[test]
    fn maps_agent_text_chunk_to_stream_delta() {
        let notif = SessionNotification::new(
            SessionId::new("acp-sid"),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("hello"),
            ))),
        );
        let out = map_session_notification(&notif, "ui-1", LlmProvider::Claude).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["kind"], "stream_delta");
        assert_eq!(v["content"], "hello");
    }
}
