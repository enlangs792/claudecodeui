//! Minimal ACP agent for bridge integration / WS E2E tests (no API keys).
//!
//! Scenarios via `MOCK_ACP_SCENARIO`:
//! - `stream` (default): thinking + multi-chunk stream + EndTurn
//! - `tools`: tool_use + tool_result + stream delta
//! - `permission`: session/request_permission then continue after client approves
//! - `slow`: 60s delay (also triggered by legacy `MOCK_ACP_SLOW=1`)
//! - `error`: prompt RPC fails with internal error

use agent_client_protocol::schema::{
    AgentCapabilities, CancelNotification, ContentBlock, ContentChunk, InitializeRequest,
    InitializeResponse, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, PromptRequest, PromptResponse, RequestPermissionRequest,
    SessionId, SessionNotification, SessionUpdate, StopReason, TextContent, ToolCall,
    ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Stdio};
use agent_client_protocol::util::internal_error;

fn scenario() -> String {
    std::env::var("MOCK_ACP_SCENARIO").unwrap_or_else(|_| "stream".to_string())
}

fn is_slow() -> bool {
    scenario() == "slow" || std::env::var("MOCK_ACP_SLOW").is_ok()
}

fn slow_delay() -> std::time::Duration {
    if std::env::var("MOCK_ACP_SLOW").is_ok() {
        std::time::Duration::from_secs(60)
    } else {
        std::time::Duration::from_secs(5)
    }
}

fn user_text(req: &PromptRequest) -> String {
    req.prompt
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn send_text_chunk(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    text: &str,
) -> agent_client_protocol::Result<()> {
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new(text),
        ))),
    ))
}

fn send_thinking_chunk(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    text: &str,
) -> agent_client_protocol::Result<()> {
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new(text),
        ))),
    ))
}

async fn run_stream_scenario(
    req: PromptRequest,
    cx: ConnectionTo<Client>,
) -> agent_client_protocol::Result<()> {
    let user = user_text(&req);
    let echo = if user.is_empty() {
        "mock-e2e-ok".to_string()
    } else {
        format!("echo: {user}")
    };

    send_thinking_chunk(&cx, &req.session_id, "thinking…")?;
    let mid = echo.len() / 2;
    let (a, b) = echo.split_at(mid.max(1).min(echo.len()));
    send_text_chunk(&cx, &req.session_id, a)?;
    send_text_chunk(&cx, &req.session_id, b)?;
    Ok(())
}

async fn run_tools_scenario(
    req: PromptRequest,
    cx: ConnectionTo<Client>,
) -> agent_client_protocol::Result<()> {
    cx.send_notification(SessionNotification::new(
        req.session_id.clone(),
        SessionUpdate::ToolCall(
            ToolCall::new(ToolCallId::new("mock-tool-1"), "Read")
                .raw_input(serde_json::json!({"path": "/tmp/mock.txt"})),
        ),
    ))?;

    cx.send_notification(SessionNotification::new(
        req.session_id.clone(),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            ToolCallId::new("mock-tool-1"),
            ToolCallUpdateFields::new()
                .raw_output(serde_json::json!({"content": "mock file contents"})),
        )),
    ))?;

    send_text_chunk(&cx, &req.session_id, "tool-done: read complete")?;
    Ok(())
}

async fn run_permission_scenario(
    req: PromptRequest,
    cx: ConnectionTo<Client>,
) -> agent_client_protocol::Result<()> {
    let _perm = cx
        .send_request_to(
            Client,
            RequestPermissionRequest::new(
                req.session_id.clone(),
                ToolCallUpdate::new(
                    ToolCallId::new("mock-perm-tool"),
                    ToolCallUpdateFields::new()
                        .title("Bash")
                        .raw_input(serde_json::json!({"command": "ls"})),
                ),
                vec![
                    PermissionOption::new(
                        "allow-once",
                        "Allow once",
                        PermissionOptionKind::AllowOnce,
                    ),
                    PermissionOption::new(
                        "reject-once",
                        "Reject",
                        PermissionOptionKind::RejectOnce,
                    ),
                ],
            ),
        )
        .block_task()
        .await?;

    send_text_chunk(&cx, &req.session_id, "permission-granted")?;
    Ok(())
}

#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    Agent
        .builder()
        .name("mock-acp-agent")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_req: NewSessionRequest, responder, _cx| {
                responder.respond(NewSessionResponse::new(SessionId::new(
                    "mock-acp-session",
                )))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, cx: ConnectionTo<Client>| {
                if is_slow() {
                    tokio::time::sleep(slow_delay()).await;
                }

                let result = match scenario().as_str() {
                    "error" => {
                        responder.respond_with_error(internal_error("mock-acp-error"))?;
                        return Ok(());
                    }
                    "tools" => run_tools_scenario(req.clone(), cx.clone()).await,
                    "permission" => run_permission_scenario(req.clone(), cx.clone()).await,
                    "slow" | "stream" | "" => run_stream_scenario(req.clone(), cx.clone()).await,
                    other => {
                        send_text_chunk(&cx, &req.session_id, &format!("unknown-scenario: {other}"))?;
                        Ok(())
                    }
                };

                if let Err(e) = result {
                    responder.respond_with_error(e)?;
                    return Ok(());
                }

                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |_cancel: CancelNotification, _cx| Ok(()),
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
}
