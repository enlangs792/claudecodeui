//! Provider traits — mirrors server/shared/interfaces.rs
//!
//! Uses `async_trait` for dyn-compatible async trait objects,
//! which is required for the multi-provider registry pattern.

use crate::shared::types::{
    FetchHistoryOptions, FetchHistoryResult, LlmProvider, McpScope, NormalizedMessage,
    ProviderAuthStatus, ProviderMcpServer, ProviderSkill, ProviderSkillListOptions,
    UpsertProviderMcpServerInput,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

// ── Main Provider Contract ──────────────────────────────────────────────────

/// Main provider contract (IProvider)
pub trait IProvider: Send + Sync {
    fn id(&self) -> LlmProvider;
    fn mcp(&self) -> Arc<dyn IProviderMcp>;
    fn auth(&self) -> Arc<dyn IProviderAuth>;
    fn skills(&self) -> Arc<dyn IProviderSkills>;
    fn sessions(&self) -> Arc<dyn IProviderSessions>;
    fn session_synchronizer(&self) -> Arc<dyn IProviderSessionSynchronizer>;
}

// ── Auth Contract ────────────────────────────────────────────────────────────

/// Authentication contract (IProviderAuth)
#[async_trait]
pub trait IProviderAuth: Send + Sync {
    /// Check if the provider is installed and authenticated
    async fn get_status(&self) -> anyhow::Result<ProviderAuthStatus>;
}

// ── Skills Contract ──────────────────────────────────────────────────────────

/// Skills contract (IProviderSkills)
#[async_trait]
pub trait IProviderSkills: Send + Sync {
    /// List all skills visible to this provider for the optional workspace
    async fn list_skills(
        &self,
        options: Option<ProviderSkillListOptions>,
    ) -> anyhow::Result<Vec<ProviderSkill>>;
}

// ── MCP Contract ────────────────────────────────────────────────────────────

/// MCP contract (IProviderMcp)
#[async_trait]
pub trait IProviderMcp: Send + Sync {
    /// List all MCP servers grouped by scope
    async fn list_servers(
        &self,
        workspace_path: Option<String>,
    ) -> anyhow::Result<HashMap<McpScope, Vec<ProviderMcpServer>>>;

    /// List servers for a specific scope
    async fn list_servers_for_scope(
        &self,
        scope: McpScope,
        workspace_path: Option<String>,
    ) -> anyhow::Result<Vec<ProviderMcpServer>>;

    /// Create or update an MCP server
    async fn upsert_server(
        &self,
        input: UpsertProviderMcpServerInput,
    ) -> anyhow::Result<ProviderMcpServer>;

    /// Remove an MCP server
    async fn remove_server(
        &self,
        name: String,
        scope: Option<McpScope>,
        workspace_path: Option<String>,
    ) -> anyhow::Result<McpRemovalResult>;
}

/// Result of removing an MCP server
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpRemovalResult {
    pub removed: bool,
    pub provider: LlmProvider,
    pub name: String,
    pub scope: McpScope,
}

// ── Session Contract ─────────────────────────────────────────────────────────

/// Session/history contract (IProviderSessions)
#[async_trait]
pub trait IProviderSessions: Send + Sync {
    /// Normalize a raw provider event into one or more NormalizedMessages
    fn normalize_message(
        &self,
        raw: serde_json::Value,
        session_id: Option<String>,
    ) -> Vec<NormalizedMessage>;

    /// Fetch paginated conversation history
    async fn fetch_history(
        &self,
        session_id: String,
        options: Option<FetchHistoryOptions>,
    ) -> anyhow::Result<FetchHistoryResult>;
}

// ── Session Synchronizer Contract ────────────────────────────────────────────

/// Session indexing contract (IProviderSessionSynchronizer)
#[async_trait]
pub trait IProviderSessionSynchronizer: Send + Sync {
    /// Scan provider session artifacts and upsert into DB
    async fn synchronize(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> anyhow::Result<usize>;

    /// Parse and upsert a single provider artifact file
    async fn synchronize_file(
        &self,
        file_path: String,
    ) -> anyhow::Result<Option<String>>;
}
