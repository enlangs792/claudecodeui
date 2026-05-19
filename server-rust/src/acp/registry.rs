//! Provider spawn command registry.

use std::str::FromStr;

use agent_client_protocol::AcpAgent;

use crate::acp::config;
use crate::shared::types::LlmProvider;

pub struct ProviderRegistry;

impl ProviderRegistry {
    pub fn resolve_agent(provider: LlmProvider) -> Result<AcpAgent, String> {
        let id = provider.as_str();
        if !config::provider_acp_enabled(id) {
            return Err(format!(
                "ACP bridge disabled for provider '{id}' (set CLOUDCLI_ACP_{}_ENABLED=1)",
                id.to_uppercase()
            ));
        }

        let cmd = config::spawn_command(id);
        let mut agent = AcpAgent::from_str(&cmd).map_err(|e| {
            format!("Invalid ACP spawn command for {id} ({cmd}): {e}")
        })?;

        if config::acp_debug_enabled() {
            let provider_id = id.to_string();
            agent = agent.with_debug(move |line, direction| {
                tracing::debug!(?direction, provider = %provider_id, line);
            });
        }

        if id == "claude" {
            if let Err(e) = crate::acp::runtime::node_resolver::ensure_node_available() {
                return Err(e);
            }
        }

        Ok(agent)
    }

    pub fn check_binary_in_path(provider: LlmProvider) -> Result<(), String> {
        let id = provider.as_str();
        if id == "claude" {
            return crate::acp::runtime::node_resolver::ensure_node_available();
        }

        let cmd = config::spawn_command(id);
        let program = cmd.split_whitespace().next().unwrap_or(&cmd);
        if which::which(program).is_err() {
            return Err(format!(
                "Required binary '{program}' not found in PATH for provider '{id}'. \
                 Install the CLI or set CLOUDCLI_ACP_{}_CMD.",
                id.to_uppercase()
            ));
        }
        Ok(())
    }
}
