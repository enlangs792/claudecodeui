//! Cursor provider (stub) — mirrors server/cursor-cli.js

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::shared::providers::*;
use crate::shared::types::*;

pub struct CursorProvider { auth: Arc<CursorAuth>, mcp: Arc<CursorMcp>, skills: Arc<CursorSkills>, sessions: Arc<CursorSessions>, synchronizer: Arc<CursorSessionSynchronizer> }
impl CursorProvider { pub fn new() -> Self { Self { auth: Arc::new(CursorAuth), mcp: Arc::new(CursorMcp), skills: Arc::new(CursorSkills), sessions: Arc::new(CursorSessions), synchronizer: Arc::new(CursorSessionSynchronizer) } } }
impl IProvider for CursorProvider {
    fn id(&self) -> LlmProvider { LlmProvider::Cursor }
    fn mcp(&self) -> Arc<dyn IProviderMcp> { self.mcp.clone() }
    fn auth(&self) -> Arc<dyn IProviderAuth> { self.auth.clone() }
    fn skills(&self) -> Arc<dyn IProviderSkills> { self.skills.clone() }
    fn sessions(&self) -> Arc<dyn IProviderSessions> { self.sessions.clone() }
    fn session_synchronizer(&self) -> Arc<dyn IProviderSessionSynchronizer> { self.synchronizer.clone() }
}

pub struct CursorAuth;
#[async_trait]
impl IProviderAuth for CursorAuth {
    async fn get_status(&self) -> anyhow::Result<ProviderAuthStatus> {
        Ok(ProviderAuthStatus { installed: std::process::Command::new("cursor-agent").arg("--version").output().map(|o| o.status.success()).unwrap_or(false), provider: LlmProvider::Cursor, authenticated: false, email: None, method: Some("token".into()), error: None })
    }
}

pub struct CursorMcp;
#[async_trait]
impl IProviderMcp for CursorMcp {
    async fn list_servers(&self, _wp: Option<String>) -> anyhow::Result<HashMap<McpScope, Vec<ProviderMcpServer>>> { Ok(HashMap::new()) }
    async fn list_servers_for_scope(&self, _: McpScope, _: Option<String>) -> anyhow::Result<Vec<ProviderMcpServer>> { Ok(Vec::new()) }
    async fn upsert_server(&self, _: UpsertProviderMcpServerInput) -> anyhow::Result<ProviderMcpServer> { Err(anyhow::anyhow!("NI")) }
    async fn remove_server(&self, n: String, s: Option<McpScope>, _: Option<String>) -> anyhow::Result<McpRemovalResult> { Ok(McpRemovalResult { removed: true, provider: LlmProvider::Cursor, name: n, scope: s.unwrap_or_default() }) }
}

pub struct CursorSkills;
#[async_trait]
impl IProviderSkills for CursorSkills { async fn list_skills(&self, _: Option<ProviderSkillListOptions>) -> anyhow::Result<Vec<ProviderSkill>> { Ok(Vec::new()) } }

pub struct CursorSessions;
#[async_trait]
impl IProviderSessions for CursorSessions {
    fn normalize_message(&self, _: Value, _: Option<String>) -> Vec<NormalizedMessage> { Vec::new() }
    async fn fetch_history(&self, _: String, _: Option<FetchHistoryOptions>) -> anyhow::Result<FetchHistoryResult> { Ok(FetchHistoryResult { messages: vec![], total: 0, has_more: false, offset: 0, limit: None, token_usage: None }) }
}

pub struct CursorSessionSynchronizer;
#[async_trait]
impl IProviderSessionSynchronizer for CursorSessionSynchronizer {
    async fn synchronize(&self, _: Option<chrono::DateTime<chrono::Utc>>) -> anyhow::Result<usize> { Ok(0) }
    async fn synchronize_file(&self, _: String) -> anyhow::Result<Option<String>> { Ok(None) }
}
