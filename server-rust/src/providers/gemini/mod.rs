//! Gemini provider (stub) — mirrors server/gemini-cli.js

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::shared::providers::*;
use crate::shared::types::*;

pub struct GeminiProvider { auth: Arc<GeminiAuth>, mcp: Arc<GeminiMcp>, skills: Arc<GeminiSkills>, sessions: Arc<GeminiSessions>, synchronizer: Arc<GeminiSessionSynchronizer> }
impl GeminiProvider { pub fn new() -> Self { Self { auth: Arc::new(GeminiAuth), mcp: Arc::new(GeminiMcp), skills: Arc::new(GeminiSkills), sessions: Arc::new(GeminiSessions), synchronizer: Arc::new(GeminiSessionSynchronizer) } } }
impl IProvider for GeminiProvider {
    fn id(&self) -> LlmProvider { LlmProvider::Gemini }
    fn mcp(&self) -> Arc<dyn IProviderMcp> { self.mcp.clone() }
    fn auth(&self) -> Arc<dyn IProviderAuth> { self.auth.clone() }
    fn skills(&self) -> Arc<dyn IProviderSkills> { self.skills.clone() }
    fn sessions(&self) -> Arc<dyn IProviderSessions> { self.sessions.clone() }
    fn session_synchronizer(&self) -> Arc<dyn IProviderSessionSynchronizer> { self.synchronizer.clone() }
}

pub struct GeminiAuth;
#[async_trait]
impl IProviderAuth for GeminiAuth {
    async fn get_status(&self) -> anyhow::Result<ProviderAuthStatus> {
        Ok(ProviderAuthStatus { installed: std::process::Command::new("gemini").arg("--version").output().map(|o| o.status.success()).unwrap_or(false), provider: LlmProvider::Gemini, authenticated: false, email: None, method: Some("oauth".into()), error: None })
    }
}

pub struct GeminiMcp;
#[async_trait]
impl IProviderMcp for GeminiMcp {
    async fn list_servers(&self, _wp: Option<String>) -> anyhow::Result<HashMap<McpScope, Vec<ProviderMcpServer>>> { Ok(HashMap::new()) }
    async fn list_servers_for_scope(&self, _: McpScope, _: Option<String>) -> anyhow::Result<Vec<ProviderMcpServer>> { Ok(Vec::new()) }
    async fn upsert_server(&self, _: UpsertProviderMcpServerInput) -> anyhow::Result<ProviderMcpServer> { Err(anyhow::anyhow!("NI")) }
    async fn remove_server(&self, n: String, s: Option<McpScope>, _: Option<String>) -> anyhow::Result<McpRemovalResult> { Ok(McpRemovalResult { removed: true, provider: LlmProvider::Gemini, name: n, scope: s.unwrap_or_default() }) }
}

pub struct GeminiSkills;
#[async_trait]
impl IProviderSkills for GeminiSkills { async fn list_skills(&self, _: Option<ProviderSkillListOptions>) -> anyhow::Result<Vec<ProviderSkill>> { Ok(Vec::new()) } }

pub struct GeminiSessions;
#[async_trait]
impl IProviderSessions for GeminiSessions {
    fn normalize_message(&self, _: Value, _: Option<String>) -> Vec<NormalizedMessage> { Vec::new() }
    async fn fetch_history(&self, _: String, _: Option<FetchHistoryOptions>) -> anyhow::Result<FetchHistoryResult> { Ok(FetchHistoryResult { messages: vec![], total: 0, has_more: false, offset: 0, limit: None, token_usage: None }) }
}

pub struct GeminiSessionSynchronizer;
#[async_trait]
impl IProviderSessionSynchronizer for GeminiSessionSynchronizer {
    async fn synchronize(&self, _: Option<chrono::DateTime<chrono::Utc>>) -> anyhow::Result<usize> { Ok(0) }
    async fn synchronize_file(&self, _: String) -> anyhow::Result<Option<String>> { Ok(None) }
}
