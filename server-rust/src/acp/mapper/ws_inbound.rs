//! Parse inbound WS chat payloads into bridge commands.

use std::path::PathBuf;

use agent_client_protocol::schema::{ContentBlock, ImageContent, TextContent};
use serde_json::Value;

use crate::shared::types::LlmProvider;

#[derive(Debug, Clone)]
pub struct ProviderCommand {
    pub provider: LlmProvider,
    pub command: String,
    pub cwd: PathBuf,
    pub ui_session_id: String,
    pub resume: bool,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub skip_permissions: bool,
    pub images: Vec<ImageAttachment>,
    pub trust: bool,
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImageAttachment {
    pub data: String,
    pub mime_type: String,
}

pub fn provider_from_msg_type(msg_type: &str) -> Option<LlmProvider> {
    match msg_type {
        "claude-command" => Some(LlmProvider::Claude),
        "cursor-command" => Some(LlmProvider::Cursor),
        "codex-command" => Some(LlmProvider::Codex),
        "gemini-command" => Some(LlmProvider::Gemini),
        _ => None,
    }
}

pub fn parse_provider_command(
    msg_type: &str,
    parsed: &Value,
) -> Option<ProviderCommand> {
    let provider = provider_from_msg_type(msg_type)?;
    let command = parsed.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let options = parsed.get("options");
    let cwd = options
        .and_then(|o| o.get("cwd").or_else(|| o.get("projectPath")))
        .and_then(|v| v.as_str())
        .unwrap_or(".")
        .to_string();

    let session_id = parsed
        .get("sessionId")
        .and_then(|v| v.as_str())
        .or_else(|| options.and_then(|o| o.get("sessionId")).and_then(|v| v.as_str()))
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let resume = options
        .and_then(|o| o.get("resume"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let model = options
        .and_then(|o| o.get("model"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let permission_mode = options
        .and_then(|o| o.get("permissionMode"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let skip_permissions = options
        .and_then(|o| o.get("skipPermissions"))
        .and_then(|v| v.as_bool())
        .or_else(|| {
            options
                .and_then(|o| o.get("toolsSettings"))
                .and_then(|t| t.get("skipPermissions"))
                .and_then(|v| v.as_bool())
        })
        .unwrap_or(false);

    let trust = options
        .and_then(|o| o.get("trust"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let sandbox_mode = options
        .and_then(|o| o.get("sandboxMode"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let images = parse_images(options);

    Some(ProviderCommand {
        provider,
        command: command.to_string(),
        cwd: PathBuf::from(cwd),
        ui_session_id: session_id,
        resume,
        model,
        permission_mode,
        skip_permissions,
        images,
        trust,
        sandbox_mode,
    })
}

pub fn parse_images(options: Option<&Value>) -> Vec<ImageAttachment> {
    let Some(options) = options else {
        return vec![];
    };
    let Some(images) = options.get("images").and_then(|v| v.as_array()) else {
        return vec![];
    };

    images
        .iter()
        .filter_map(|img| {
            let data = img
                .get("data")
                .or_else(|| img.get("base64"))
                .and_then(|v| v.as_str())?;
            let mime_type = img
                .get("mimeType")
                .or_else(|| img.get("mime_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("image/png")
                .to_string();
            Some(ImageAttachment {
                data: data.to_string(),
                mime_type,
            })
        })
        .collect()
}

pub fn build_content_blocks(cmd: &ProviderCommand) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    if !cmd.command.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new(cmd.command.clone())));
    }
    for image in &cmd.images {
        blocks.push(ContentBlock::Image(ImageContent::new(
            &image.data,
            &image.mime_type,
        )));
    }
    if blocks.is_empty() {
        blocks.push(ContentBlock::Text(TextContent::new("")));
    }
    blocks
}

pub fn parse_permission_response(parsed: &Value) -> Option<(String, crate::acp::permissions::PermissionDecision)> {
    let request_id = parsed.get("requestId").and_then(|v| v.as_str())?;
    let allow = parsed.get("allow").and_then(|v| v.as_bool()).unwrap_or(false);
    let updated_input = parsed.get("updatedInput").cloned();
    let message = parsed
        .get("message")
        .and_then(|v| v.as_str())
        .map(String::from);
    Some((
        request_id.to_string(),
        crate::acp::permissions::PermissionDecision {
            allow,
            updated_input,
            message,
        },
    ))
}
