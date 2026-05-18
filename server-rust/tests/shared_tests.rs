//! Unit tests for shared types, utils, network, and model constants

use cloudcli_server::shared::types::*;
use cloudcli_server::shared::{network, model_constants, utils};

// ── LlmProvider ─────────────────────────────────────────────────────────────

#[test]
fn test_llm_provider_serde_lowercase() {
    assert_eq!(serde_json::to_string(&LlmProvider::Claude).unwrap(), "\"claude\"");
    assert_eq!(serde_json::to_string(&LlmProvider::Codex).unwrap(), "\"codex\"");
    assert_eq!(serde_json::to_string(&LlmProvider::Gemini).unwrap(), "\"gemini\"");
    assert_eq!(serde_json::to_string(&LlmProvider::Cursor).unwrap(), "\"cursor\"");
}

#[test]
fn test_llm_provider_deser() {
    assert_eq!(serde_json::from_str::<LlmProvider>("\"claude\"").unwrap(), LlmProvider::Claude);
    assert_eq!(serde_json::from_str::<LlmProvider>("\"codex\"").unwrap(), LlmProvider::Codex);
}

#[test]
fn test_llm_provider_default() {
    assert_eq!(LlmProvider::default(), LlmProvider::Claude);
}

// ── MessageKind ──────────────────────────────────────────────────────────────

#[test]
fn test_message_kind_serde_snake_case() {
    assert_eq!(serde_json::to_string(&MessageKind::ToolUse).unwrap(), "\"tool_use\"");
    assert_eq!(serde_json::to_string(&MessageKind::ToolResult).unwrap(), "\"tool_result\"");
    assert_eq!(serde_json::to_string(&MessageKind::PermissionRequest).unwrap(), "\"permission_request\"");
}

#[test]
fn test_message_kind_deser() {
    assert_eq!(serde_json::from_str::<MessageKind>("\"tool_use\"").unwrap(), MessageKind::ToolUse);
    assert_eq!(serde_json::from_str::<MessageKind>("\"session_created\"").unwrap(), MessageKind::SessionCreated);
}

#[test]
fn test_message_kind_default() {
    assert_eq!(MessageKind::default(), MessageKind::Text);
}

// ── McpScope / McpTransport ──────────────────────────────────────────────────

#[test]
fn test_mcp_scope_serde() {
    assert_eq!(serde_json::to_string(&McpScope::User).unwrap(), "\"user\"");
    assert_eq!(serde_json::to_string(&McpScope::Local).unwrap(), "\"local\"");
    assert_eq!(serde_json::to_string(&McpScope::Project).unwrap(), "\"project\"");
}

#[test]
fn test_mcp_scope_default() {
    assert_eq!(McpScope::default(), McpScope::User);
}

#[test]
fn test_mcp_transport_serde() {
    assert_eq!(serde_json::to_string(&McpTransport::Stdio).unwrap(), "\"stdio\"");
    assert_eq!(serde_json::to_string(&McpTransport::Http).unwrap(), "\"http\"");
    assert_eq!(serde_json::to_string(&McpTransport::Sse).unwrap(), "\"sse\"");
}

#[test]
fn test_mcp_transport_default() {
    assert_eq!(McpTransport::default(), McpTransport::Stdio);
}

// ── NormalizedMessage camelCase ──────────────────────────────────────────────

#[test]
fn test_normalized_message_camelcase_fields() {
    let mut extra = serde_json::Map::new();
    extra.insert("custom".into(), serde_json::json!("value"));

    let msg = NormalizedMessage {
        id: "test-1".into(),
        session_id: "sess-1".into(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        provider: LlmProvider::Claude,
        kind: MessageKind::Text,
        content: Some("hello".into()),
        is_error: Some(false),
        is_local_command: Some(true),
        extra,
        ..Default::default()
    };
    let json_str = serde_json::to_string(&msg).unwrap();
    assert!(json_str.contains("sessionId"));
    assert!(json_str.contains("isError"));
    assert!(json_str.contains("isLocalCommand"));
    assert!(!json_str.contains("\"session_id\""));
}

// ── Network helpers ──────────────────────────────────────────────────────────

#[test]
fn test_is_wildcard_host() {
    assert!(network::is_wildcard_host("0.0.0.0"));
    assert!(network::is_wildcard_host("::"));
    assert!(!network::is_wildcard_host("127.0.0.1"));
}

#[test]
fn test_is_loopback_host() {
    assert!(network::is_loopback_host("localhost"));
    assert!(network::is_loopback_host("127.0.0.1"));
    assert!(!network::is_loopback_host("example.com"));
}

#[test]
fn test_get_connectable_host() {
    assert_eq!(network::get_connectable_host(""), "localhost");
    assert_eq!(network::get_connectable_host("0.0.0.0"), "localhost");
    assert_eq!(network::get_connectable_host("localhost"), "localhost");
    assert_eq!(network::get_connectable_host("cloudcli.ai"), "cloudcli.ai");
}

// ── Path normalization ───────────────────────────────────────────────────────

#[test]
fn test_normalize_project_path_trims() {
    // normalize_project_path trims whitespace, then normalizes the path
    let result = utils::normalize_project_path("  /home/user/project  ");
    assert!(result.ends_with("/home/user/project"));
    assert!(!result.starts_with(" "));
}

#[test]
fn test_normalize_project_path_empty() {
    assert_eq!(utils::normalize_project_path(""), "");
    assert_eq!(utils::normalize_project_path("   "), "");
}

#[test]
fn test_normalize_project_path_root() {
    assert_eq!(utils::normalize_project_path("/"), "/");
}

#[test]
fn test_normalize_project_path_strips_trailing_slash() {
    let result = utils::normalize_project_path("/home/user/");
    assert!(!result.ends_with('/') || result == "/");
    assert!(result.starts_with('/'));
}

// ── Message ID generation ────────────────────────────────────────────────────

#[test]
fn test_generate_message_id_has_prefix() {
    let id = utils::generate_message_id("test");
    assert!(id.starts_with("test_"));
    assert!(id.len() > 6);
}

#[test]
fn test_generate_message_id_unique() {
    let id1 = utils::generate_message_id("a");
    let id2 = utils::generate_message_id("a");
    assert_ne!(id1, id2);
}

// ── JSON helpers ─────────────────────────────────────────────────────────────

#[test]
fn test_read_optional_string_some() {
    let v = serde_json::json!("hello");
    assert_eq!(utils::read_optional_string(&v).unwrap(), "hello");
}

#[test]
fn test_read_optional_string_none_for_number() {
    assert_eq!(utils::read_optional_string(&serde_json::json!(42)), None);
}

#[test]
fn test_read_optional_string_trims() {
    let v = serde_json::json!("  hello  ");
    assert_eq!(utils::read_optional_string(&v).unwrap(), "hello");
}

#[test]
fn test_read_string_array_some() {
    let v = serde_json::json!(["a", "b", "c"]);
    assert_eq!(utils::read_string_array(&v).unwrap(), vec!["a", "b", "c"]);
}

#[test]
fn test_read_string_array_filters_non_strings() {
    let v = serde_json::json!(["a", 42, "c"]);
    assert_eq!(utils::read_string_array(&v).unwrap(), vec!["a", "c"]);
}

#[test]
fn test_read_string_array_none_for_scalar() {
    assert_eq!(utils::read_string_array(&serde_json::json!("nope")), None);
}

#[test]
fn test_read_object_record_some() {
    let v = serde_json::json!({"key": "value"});
    let result = utils::read_object_record(&v).unwrap();
    assert_eq!(result.get("key").unwrap().as_str().unwrap(), "value");
}

#[test]
fn test_read_object_record_rejects_array() {
    assert_eq!(utils::read_object_record(&serde_json::json!([1, 2])), None);
}

#[test]
fn test_read_object_record_rejects_primitive() {
    assert_eq!(utils::read_object_record(&serde_json::json!("str")), None);
    assert_eq!(utils::read_object_record(&serde_json::json!(null)), None);
}

// ── Session name normalization ───────────────────────────────────────────────

#[test]
fn test_normalize_session_name_collapses_whitespace() {
    assert_eq!(
        utils::normalize_session_name(Some("  hello   world  "), "fallback"),
        "hello world"
    );
}

#[test]
fn test_normalize_session_name_truncates() {
    let long = "a".repeat(200);
    let result = utils::normalize_session_name(Some(&long), "fallback");
    assert_eq!(result.len(), 120);
}

#[test]
fn test_normalize_session_name_fallback() {
    assert_eq!(utils::normalize_session_name(None, "fallback"), "fallback");
    assert_eq!(utils::normalize_session_name(Some(""), "fallback"), "fallback");
}

// ── Model constants ──────────────────────────────────────────────────────────

#[test]
fn test_claude_models_default() {
    assert_eq!(model_constants::claude_models().default, "opus");
}

#[test]
fn test_codex_models_default() {
    assert_eq!(model_constants::codex_models().default, "gpt-5.4");
}

#[test]
fn test_cursor_models_many() {
    let models = model_constants::cursor_models();
    assert!(models.options.len() > 10);
    assert_eq!(models.default, "gpt-5.3-codex");
}

#[test]
fn test_gemini_models_default() {
    assert_eq!(model_constants::gemini_models().default, "gemini-3.1-pro-preview");
}

#[test]
fn test_providers_registry_has_four() {
    let registry = model_constants::providers();
    assert_eq!(registry.len(), 4);
    let ids: Vec<&str> = registry.iter().map(|r| r.id.as_str()).collect();
    for expected in &["claude", "codex", "gemini", "cursor"] {
        assert!(ids.contains(expected), "missing provider: {}", expected);
    }
}
