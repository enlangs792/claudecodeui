//! Codex provider (stub) — mirrors server/openai-codex.js

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::shared::providers::*;
use crate::shared::types::*;

pub struct CodexProvider {
    auth: Arc<CodexAuth>,
    mcp: Arc<CodexMcp>,
    skills: Arc<CodexSkills>,
    sessions: Arc<CodexSessions>,
    synchronizer: Arc<CodexSessionSynchronizer>,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            auth: Arc::new(CodexAuth),
            mcp: Arc::new(CodexMcp),
            skills: Arc::new(CodexSkills),
            sessions: Arc::new(CodexSessions),
            synchronizer: Arc::new(CodexSessionSynchronizer),
        }
    }
}

impl IProvider for CodexProvider {
    fn id(&self) -> LlmProvider { LlmProvider::Codex }
    fn mcp(&self) -> Arc<dyn IProviderMcp> { self.mcp.clone() }
    fn auth(&self) -> Arc<dyn IProviderAuth> { self.auth.clone() }
    fn skills(&self) -> Arc<dyn IProviderSkills> { self.skills.clone() }
    fn sessions(&self) -> Arc<dyn IProviderSessions> { self.sessions.clone() }
    fn session_synchronizer(&self) -> Arc<dyn IProviderSessionSynchronizer> { self.synchronizer.clone() }
}

pub struct CodexAuth;
#[async_trait]
impl IProviderAuth for CodexAuth {
    async fn get_status(&self) -> anyhow::Result<ProviderAuthStatus> {
        Ok(ProviderAuthStatus {
            installed: std::process::Command::new("codex").arg("--version").output().map(|o| o.status.success()).unwrap_or(false),
            provider: LlmProvider::Codex,
            authenticated: dirs::home_dir().map(|h| h.join(".codex").join("credentials.json").exists()).unwrap_or(false),
            email: None, method: Some("oauth".into()), error: None,
        })
    }
}

pub struct CodexMcp;
#[async_trait]
impl IProviderMcp for CodexMcp {
    async fn list_servers(&self, _wp: Option<String>) -> anyhow::Result<HashMap<McpScope, Vec<ProviderMcpServer>>> {
        Ok(HashMap::new())
    }
    async fn list_servers_for_scope(&self, _scope: McpScope, _wp: Option<String>) -> anyhow::Result<Vec<ProviderMcpServer>> {
        Ok(Vec::new())
    }
    async fn upsert_server(&self, _input: UpsertProviderMcpServerInput) -> anyhow::Result<ProviderMcpServer> {
        Err(anyhow::anyhow!("Not implemented"))
    }
    async fn remove_server(&self, name: String, scope: Option<McpScope>, _wp: Option<String>) -> anyhow::Result<McpRemovalResult> {
        Ok(McpRemovalResult { removed: true, provider: LlmProvider::Codex, name, scope: scope.unwrap_or_default() })
    }
}

pub struct CodexSkills;
#[async_trait]
impl IProviderSkills for CodexSkills {
    async fn list_skills(&self, _options: Option<ProviderSkillListOptions>) -> anyhow::Result<Vec<ProviderSkill>> {
        Ok(Vec::new())
    }
}

pub struct CodexSessions;
#[async_trait]
impl IProviderSessions for CodexSessions {
    fn normalize_message(&self, _raw: Value, _session_id: Option<String>) -> Vec<NormalizedMessage> {
        Vec::new()
    }
    async fn fetch_history(&self, _session_id: String, _options: Option<FetchHistoryOptions>) -> anyhow::Result<FetchHistoryResult> {
        Ok(FetchHistoryResult { messages: vec![], total: 0, has_more: false, offset: 0, limit: None, token_usage: None })
    }
}

pub struct CodexSessionSynchronizer;
#[async_trait]
impl IProviderSessionSynchronizer for CodexSessionSynchronizer {
    async fn synchronize(&self, _since: Option<chrono::DateTime<chrono::Utc>>) -> anyhow::Result<usize> { Ok(0) }
    async fn synchronize_file(&self, _file_path: String) -> anyhow::Result<Option<String>> { Ok(None) }
}
