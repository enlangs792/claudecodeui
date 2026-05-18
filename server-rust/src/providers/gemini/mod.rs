//! Gemini provider — mirrors server/gemini-cli.js

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::shared::providers::*;
use crate::shared::types::*;

// ── Gemini Provider ─────────────────────────────────────────────────────────

pub struct GeminiProvider {
    auth: Arc<GeminiAuth>,
    mcp: Arc<GeminiMcp>,
    skills: Arc<GeminiSkills>,
    sessions: Arc<GeminiSessions>,
    synchronizer: Arc<GeminiSessionSynchronizer>,
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self {
            auth: Arc::new(GeminiAuth),
            mcp: Arc::new(GeminiMcp),
            skills: Arc::new(GeminiSkills),
            sessions: Arc::new(GeminiSessions),
            synchronizer: Arc::new(GeminiSessionSynchronizer),
        }
    }
}

impl IProvider for GeminiProvider {
    fn id(&self) -> LlmProvider { LlmProvider::Gemini }
    fn mcp(&self) -> Arc<dyn IProviderMcp> { self.mcp.clone() }
    fn auth(&self) -> Arc<dyn IProviderAuth> { self.auth.clone() }
    fn skills(&self) -> Arc<dyn IProviderSkills> { self.skills.clone() }
    fn sessions(&self) -> Arc<dyn IProviderSessions> { self.sessions.clone() }
    fn session_synchronizer(&self) -> Arc<dyn IProviderSessionSynchronizer> { self.synchronizer.clone() }
}

// ── Auth ─────────────────────────────────────────────────────────────────────

pub struct GeminiAuth;

#[async_trait]
impl IProviderAuth for GeminiAuth {
    async fn get_status(&self) -> anyhow::Result<ProviderAuthStatus> {
        let installed = std::process::Command::new("gemini")
            .arg("--version")
            .output()
            .is_ok();
        let home = dirs::home_dir();
        let authenticated = home.as_ref().map(|h| h.join(".gemini").exists()).unwrap_or(false);

        Ok(ProviderAuthStatus {
            installed,
            provider: LlmProvider::Gemini,
            authenticated,
            email: None,
            method: Some("oauth".into()),
            error: None,
        })
    }
}

// ── MCP ──────────────────────────────────────────────────────────────────────

pub struct GeminiMcp;

#[async_trait]
impl IProviderMcp for GeminiMcp {
    async fn list_servers(&self, _workspace_path: Option<String>) -> anyhow::Result<HashMap<McpScope, Vec<ProviderMcpServer>>> {
        Ok(HashMap::new())
    }

    async fn list_servers_for_scope(&self, _scope: McpScope, _workspace_path: Option<String>) -> anyhow::Result<Vec<ProviderMcpServer>> {
        Ok(Vec::new())
    }

    async fn upsert_server(&self, _input: UpsertProviderMcpServerInput) -> anyhow::Result<ProviderMcpServer> {
        Err(anyhow::anyhow!("MCP not supported for Gemini"))
    }

    async fn remove_server(
        &self,
        name: String,
        scope: Option<McpScope>,
        _workspace_path: Option<String>,
    ) -> anyhow::Result<McpRemovalResult> {
        Ok(McpRemovalResult {
            removed: true,
            provider: LlmProvider::Gemini,
            name,
            scope: scope.unwrap_or(McpScope::User),
        })
    }
}

// ── Skills ───────────────────────────────────────────────────────────────────

pub struct GeminiSkills;

#[async_trait]
impl IProviderSkills for GeminiSkills {
    async fn list_skills(&self, _options: Option<ProviderSkillListOptions>) -> anyhow::Result<Vec<ProviderSkill>> {
        Ok(Vec::new())
    }
}

// ── Sessions ─────────────────────────────────────────────────────────────────

pub struct GeminiSessions;

#[async_trait]
impl IProviderSessions for GeminiSessions {
    fn normalize_message(&self, raw: Value, session_id: Option<String>) -> Vec<NormalizedMessage> {
        let kind = match raw.get("role").and_then(|v| v.as_str()) {
            Some("user") | Some("User") => MessageKind::Text,
            Some("model") | Some("assistant") | Some("Model") | Some("Assistant") => MessageKind::Text,
            Some("tool_use") | Some("tool_use_msg") => MessageKind::ToolUse,
            Some("tool_result") | Some("tool_result_msg") => MessageKind::ToolResult,
            _ => {
                // Fall back to type field if role is not present
                match raw.get("type").and_then(|v| v.as_str()) {
                    Some("user") | Some("user_message") => MessageKind::Text,
                    Some("assistant") | Some("assistant_message") => MessageKind::Text,
                    Some("tool_use") => MessageKind::ToolUse,
                    Some("tool_result") => MessageKind::ToolResult,
                    _ => MessageKind::Text,
                }
            }
        };

        let content = raw
            .get("content")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| {
                // Gemini sometimes uses "parts" array with "text" sub-fields
                raw.get("parts")
                    .and_then(|p| p.as_array())
                    .and_then(|arr| {
                        let text: String = arr
                            .iter()
                            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n");
                        if text.is_empty() { None } else { Some(text) }
                    })
            });

        vec![NormalizedMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.unwrap_or_default(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            provider: LlmProvider::Gemini,
            kind,
            content,
            ..Default::default()
        }]
    }

    async fn fetch_history(
        &self,
        session_id: String,
        _options: Option<FetchHistoryOptions>,
    ) -> anyhow::Result<FetchHistoryResult> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
        let sessions_dir = home.join(".gemini").join("sessions");

        let mut messages: Vec<NormalizedMessage> = Vec::new();
        let session_file = sessions_dir.join(format!("{}.jsonl", session_id));
        if let Ok(content) = tokio::fs::read_to_string(&session_file).await {
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(raw) = serde_json::from_str::<Value>(line.trim()) {
                    messages.extend(self.normalize_message(raw, Some(session_id.clone())));
                }
            }
        }

        let total = messages.len() as u32;
        Ok(FetchHistoryResult {
            messages,
            total,
            has_more: false,
            offset: 0,
            limit: None,
            token_usage: None,
        })
    }
}

// ── Session Synchronizer ─────────────────────────────────────────────────────

pub struct GeminiSessionSynchronizer;

#[async_trait]
impl IProviderSessionSynchronizer for GeminiSessionSynchronizer {
    async fn synchronize(&self, since: Option<chrono::DateTime<chrono::Utc>>) -> anyhow::Result<usize> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home not found"))?;
        let sessions_dir = home.join(".gemini").join("sessions");

        let jsonl_files = crate::shared::utils::find_files_recursively_created_after(
            &sessions_dir, ".jsonl", since,
        )
        .await;

        let count = jsonl_files.len();
        for file in &jsonl_files {
            self.synchronize_file(file.clone()).await.ok();
        }

        Ok(count)
    }

    async fn synchronize_file(&self, file_path: String) -> anyhow::Result<Option<String>> {
        let path = std::path::Path::new(&file_path);
        let session_id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");

        crate::db::repos::sessions::SessionsRepo::upsert(
            session_id,
            "gemini",
            &std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .to_string_lossy(),
            None,
            Some(&file_path),
        );

        Ok(Some(session_id.to_string()))
    }
}
