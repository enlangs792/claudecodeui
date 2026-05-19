//! Environment configuration for the ACP bridge.

/// Returns true when `CLOUDCLI_ACP_BRIDGE` is not explicitly disabled.
pub fn acp_bridge_enabled() -> bool {
    match std::env::var("CLOUDCLI_ACP_BRIDGE").as_deref() {
        Ok("0") | Ok("false") | Ok("no") => false,
        _ => true,
    }
}

pub fn acp_debug_enabled() -> bool {
    matches!(
        std::env::var("CLOUDCLI_ACP_DEBUG").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

pub fn spawn_command(provider: &str) -> String {
    let key = format!(
        "CLOUDCLI_ACP_{}_CMD",
        provider.to_uppercase()
    );
    std::env::var(&key).unwrap_or_else(|_| default_spawn_command(provider).to_string())
}

fn default_spawn_command(provider: &str) -> &'static str {
    match provider {
        "claude" => "npx -y @agentclientprotocol/claude-agent-acp",
        "gemini" => "gemini --acp",
        "cursor" => "agent acp",
        "codex" => "codex-acp",
        _ => "npx -y @agentclientprotocol/claude-agent-acp",
    }
}

pub fn provider_acp_enabled(provider: &str) -> bool {
    let key = format!(
        "CLOUDCLI_ACP_{}_ENABLED",
        provider.to_uppercase()
    );
    match std::env::var(&key).as_deref() {
        Ok("0") | Ok("false") | Ok("no") => false,
        _ => true,
    }
}

pub fn node_path_override() -> Option<String> {
    std::env::var("CLOUDCLI_NODE_PATH").ok().filter(|s| !s.is_empty())
}

pub fn managed_node_enabled() -> bool {
    matches!(
        std::env::var("CLOUDCLI_MANAGED_NODE").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}
