//! Cursor provider — mirrors server/cursor-cli.js

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::shared::providers::*;
use crate::shared::types::*;

// ── Cursor Provider ─────────────────────────────────────────────────────────

pub struct CursorProvider {
    auth: Arc<CursorAuth>,
    mcp: Arc<CursorMcp>,
    skills: Arc<CursorSkills>,
    sessions: Arc<CursorSessions>,
    synchronizer: Arc<CursorSessionSynchronizer>,
}

impl CursorProvider {
    pub fn new() -> Self {
        Self {
            auth: Arc::new(CursorAuth),
            mcp: Arc::new(CursorMcp),
            skills: Arc::new(CursorSkills),
            sessions: Arc::new(CursorSessions),
            synchronizer: Arc::new(CursorSessionSynchronizer),
        }
    }
}

impl IProvider for CursorProvider {
    fn id(&self) -> LlmProvider { LlmProvider::Cursor }
    fn mcp(&self) -> Arc<dyn IProviderMcp> { self.mcp.clone() }
    fn auth(&self) -> Arc<dyn IProviderAuth> { self.auth.clone() }
    fn skills(&self) -> Arc<dyn IProviderSkills> { self.skills.clone() }
    fn sessions(&self) -> Arc<dyn IProviderSessions> { self.sessions.clone() }
    fn session_synchronizer(&self) -> Arc<dyn IProviderSessionSynchronizer> { self.synchronizer.clone() }
}

// ── Auth ─────────────────────────────────────────────────────────────────────

pub struct CursorAuth;

#[async_trait]
impl IProviderAuth for CursorAuth {
    async fn get_status(&self) -> anyhow::Result<ProviderAuthStatus> {
        let installed = std::process::Command::new("cursor-agent")
            .arg("--version")
            .output()
            .is_ok();
        let home = dirs::home_dir();
        let authenticated = home
            .as_ref()
            .map(|h| h.join(".cursor").join("credentials.json").exists())
            .unwrap_or(false);

        Ok(ProviderAuthStatus {
            installed,
            provider: LlmProvider::Cursor,
            authenticated,
            email: None,
            method: Some("token".into()),
            error: None,
        })
    }
}

// ── MCP ──────────────────────────────────────────────────────────────────────

pub struct CursorMcp;

#[async_trait]
impl IProviderMcp for CursorMcp {
    async fn list_servers(
        &self,
        workspace_path: Option<String>,
    ) -> anyhow::Result<HashMap<McpScope, Vec<ProviderMcpServer>>> {
        let user_servers = self.list_servers_for_scope(McpScope::User, workspace_path.clone()).await?;
        let project_servers = self.list_servers_for_scope(McpScope::Project, workspace_path.clone()).await?;

        let mut map: HashMap<McpScope, Vec<ProviderMcpServer>> = HashMap::new();
        map.insert(McpScope::User, user_servers);
        map.insert(McpScope::Project, project_servers);
        Ok(map)
    }

    async fn list_servers_for_scope(
        &self,
        scope: McpScope,
        _workspace_path: Option<String>,
    ) -> anyhow::Result<Vec<ProviderMcpServer>> {
        let config_path = match scope {
            McpScope::User => dirs::home_dir().map(|h| h.join(".cursor").join("mcp.json")),
            McpScope::Project => None,
            McpScope::Local => None,
        };

        let Some(path) = config_path else {
            return Ok(Vec::new());
        };

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let config: Value = serde_json::from_str(&content).unwrap_or_default();
                Ok(parse_cursor_mcp_config(&config, scope))
            }
            Err(_) => Ok(Vec::new()),
        }
    }

    async fn upsert_server(&self, _input: UpsertProviderMcpServerInput) -> anyhow::Result<ProviderMcpServer> {
        Err(anyhow::anyhow!("MCP upsert not implemented for Cursor"))
    }

    async fn remove_server(
        &self,
        name: String,
        scope: Option<McpScope>,
        _workspace_path: Option<String>,
    ) -> anyhow::Result<McpRemovalResult> {
        Ok(McpRemovalResult {
            removed: true,
            provider: LlmProvider::Cursor,
            name,
            scope: scope.unwrap_or(McpScope::User),
        })
    }
}

fn parse_cursor_mcp_config(config: &Value, scope: McpScope) -> Vec<ProviderMcpServer> {
    let Some(servers_obj) = config.get("mcpServers").and_then(|v| v.as_object()) else {
        return Vec::new();
    };

    let mut servers = Vec::new();
    for (name, server_config) in servers_obj {
        let transport_str = server_config.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
        let transport = match transport_str {
            "sse" => McpTransport::Sse,
            "http" => McpTransport::Http,
            _ => McpTransport::Stdio,
        };

        let server = ProviderMcpServer {
            provider: LlmProvider::Cursor,
            name: name.clone(),
            scope,
            transport,
            command: server_config.get("command").and_then(|v| v.as_str()).map(String::from),
            args: server_config.get("args").and_then(|v| v.as_array()).map(|a| {
                a.iter().filter_map(|v| v.as_str().map(String::from)).collect()
            }),
            env: server_config.get("env").and_then(|v| v.as_object()).map(|o| {
                let mut map = serde_json::Map::new();
                for (k, v) in o {
                    map.insert(k.clone(), v.clone());
                }
                map
            }),
            url: server_config.get("url").and_then(|v| v.as_str()).map(String::from),
            ..Default::default()
        };
        servers.push(server);
    }

    servers
}

// ── Skills ───────────────────────────────────────────────────────────────────

pub struct CursorSkills;

#[async_trait]
impl IProviderSkills for CursorSkills {
    async fn list_skills(&self, _options: Option<ProviderSkillListOptions>) -> anyhow::Result<Vec<ProviderSkill>> {
        Ok(Vec::new())
    }
}

// ── Sessions ─────────────────────────────────────────────────────────────────

pub struct CursorSessions;

#[async_trait]
impl IProviderSessions for CursorSessions {
    fn normalize_message(&self, raw: Value, session_id: Option<String>) -> Vec<NormalizedMessage> {
        let kind = match raw.get("type").and_then(|v| v.as_str()) {
            Some("user") | Some("user_message") | Some("UserMessage") => MessageKind::Text,
            Some("assistant") | Some("assistant_message") | Some("AssistantMessage") => MessageKind::Text,
            Some("tool_use") | Some("ToolUse") => MessageKind::ToolUse,
            Some("tool_result") | Some("ToolResult") => MessageKind::ToolResult,
            _ => {
                // Fall back to role field
                match raw.get("role").and_then(|v| v.as_str()) {
                    Some("user") | Some("User") => MessageKind::Text,
                    Some("assistant") | Some("Assistant") => MessageKind::Text,
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
                raw.get("text")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });

        vec![NormalizedMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.unwrap_or_default(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            provider: LlmProvider::Cursor,
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
        let sessions_dir = home.join(".cursor").join("sessions");

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

pub struct CursorSessionSynchronizer;

#[async_trait]
impl IProviderSessionSynchronizer for CursorSessionSynchronizer {
    async fn synchronize(&self, since: Option<chrono::DateTime<chrono::Utc>>) -> anyhow::Result<usize> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home not found"))?;
        let sessions_dir = home.join(".cursor").join("sessions");

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
            "cursor",
            &std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .to_string_lossy(),
            None,
            Some(&file_path),
        );

        Ok(Some(session_id.to_string()))
    }
}
