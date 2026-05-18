//! Claude provider — mirrors server/modules/providers/list/claude/

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::shared::providers::{
    IProvider, IProviderAuth, IProviderMcp, IProviderSkills, IProviderSessions,
    IProviderSessionSynchronizer, McpRemovalResult,
};
use crate::shared::types::{
    FetchHistoryOptions, FetchHistoryResult, LlmProvider, McpScope, McpTransport,
    NormalizedMessage, ProviderAuthStatus, ProviderMcpServer, ProviderSkill,
    ProviderSkillListOptions, ProviderSkillScope, UpsertProviderMcpServerInput,
};

// ── Claude Provider ──────────────────────────────────────────────────────────

pub struct ClaudeProvider {
    auth: Arc<ClaudeAuth>,
    mcp: Arc<ClaudeMcp>,
    skills: Arc<ClaudeSkills>,
    sessions: Arc<ClaudeSessions>,
    synchronizer: Arc<ClaudeSessionSynchronizer>,
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            auth: Arc::new(ClaudeAuth),
            mcp: Arc::new(ClaudeMcp),
            skills: Arc::new(ClaudeSkills),
            sessions: Arc::new(ClaudeSessions),
            synchronizer: Arc::new(ClaudeSessionSynchronizer),
        }
    }
}

impl IProvider for ClaudeProvider {
    fn id(&self) -> LlmProvider {
        LlmProvider::Claude
    }
    fn mcp(&self) -> Arc<dyn IProviderMcp> { self.mcp.clone() }
    fn auth(&self) -> Arc<dyn IProviderAuth> { self.auth.clone() }
    fn skills(&self) -> Arc<dyn IProviderSkills> { self.skills.clone() }
    fn sessions(&self) -> Arc<dyn IProviderSessions> { self.sessions.clone() }
    fn session_synchronizer(&self) -> Arc<dyn IProviderSessionSynchronizer> { self.synchronizer.clone() }
}

// ── Auth ─────────────────────────────────────────────────────────────────────

pub struct ClaudeAuth;

#[async_trait]
impl IProviderAuth for ClaudeAuth {
    async fn get_status(&self) -> anyhow::Result<ProviderAuthStatus> {
        let installed = check_claude_installed();
        let authenticated = if installed {
            check_claude_login().await.unwrap_or(false)
        } else {
            false
        };

        Ok(ProviderAuthStatus {
            installed,
            provider: LlmProvider::Claude,
            authenticated,
            email: None,
            method: Some("oauth".into()),
            error: None,
        })
    }
}

fn check_claude_installed() -> bool {
    std::process::Command::new("claude")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn check_claude_login() -> Option<bool> {
    let home = dirs::home_dir()?;
    let credentials_path = home.join(".claude").join("credentials.json");
    tokio::fs::metadata(&credentials_path).await.ok().map(|_| true)
}

// ── MCP ──────────────────────────────────────────────────────────────────────

pub struct ClaudeMcp;

#[async_trait]
impl IProviderMcp for ClaudeMcp {
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
        workspace_path: Option<String>,
    ) -> anyhow::Result<Vec<ProviderMcpServer>> {
        let config_path = match scope {
            McpScope::User => dirs::home_dir().map(|h| h.join(".claude").join(".mcp.json")),
            McpScope::Project => workspace_path.map(|p| PathBuf::from(p).join(".mcp.json")),
            McpScope::Local => None,
        };

        let Some(path) = config_path else {
            return Ok(Vec::new());
        };

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let config: Value = serde_json::from_str(&content).unwrap_or_default();
                parse_mcp_config(&config, scope)
            }
            Err(_) => Ok(Vec::new()),
        }
    }

    async fn upsert_server(&self, _input: UpsertProviderMcpServerInput) -> anyhow::Result<ProviderMcpServer> {
        Err(anyhow::anyhow!("MCP upsert not implemented for Claude"))
    }

    async fn remove_server(
        &self,
        name: String,
        scope: Option<McpScope>,
        _workspace_path: Option<String>,
    ) -> anyhow::Result<McpRemovalResult> {
        Ok(McpRemovalResult {
            removed: true,
            provider: LlmProvider::Claude,
            name,
            scope: scope.unwrap_or(McpScope::User),
        })
    }
}

fn parse_mcp_config(config: &Value, scope: McpScope) -> anyhow::Result<Vec<ProviderMcpServer>> {
    let Some(servers_obj) = config.get("mcpServers").and_then(|v| v.as_object()) else {
        return Ok(Vec::new());
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
            provider: LlmProvider::Claude,
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

    Ok(servers)
}

// ── Skills ───────────────────────────────────────────────────────────────────

pub struct ClaudeSkills;

#[async_trait]
impl IProviderSkills for ClaudeSkills {
    async fn list_skills(
        &self,
        options: Option<ProviderSkillListOptions>,
    ) -> anyhow::Result<Vec<ProviderSkill>> {
        let mut skills = Vec::new();

        // User skills: ~/.claude/skills/
        if let Some(home) = dirs::home_dir() {
            let user_skills_dir = home.join(".claude").join("skills");
            discover_skills_in_dir(&user_skills_dir, ProviderSkillScope::User, &mut skills).await;
        }

        // Project skills: <workspace>/.claude/skills/
        if let Some(opts) = options {
            if let Some(ws) = opts.workspace_path {
                let project_skills_dir = PathBuf::from(&ws).join(".claude").join("skills");
                discover_skills_in_dir(&project_skills_dir, ProviderSkillScope::Project, &mut skills).await;
            }
        }

        Ok(skills)
    }
}

async fn discover_skills_in_dir(
    dir: &PathBuf,
    scope: ProviderSkillScope,
    skills: &mut Vec<ProviderSkill>,
) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let ft = match entry.file_type().await {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !ft.is_dir() {
            continue;
        }
        let skill_md = entry.path().join("SKILL.md");
        if tokio::fs::metadata(&skill_md).await.map(|m| m.is_file()).unwrap_or(false) {
            let name = entry.file_name().to_string_lossy().into_owned();
            skills.push(ProviderSkill {
                provider: LlmProvider::Claude,
                name: name.clone(),
                description: format!("Custom skill: {}", name),
                command: format!("/{}", name),
                scope,
                source_path: skill_md.to_string_lossy().into_owned(),
                plugin_name: None,
                plugin_id: None,
            });
        }
    }
}

// ── Sessions ─────────────────────────────────────────────────────────────────

pub struct ClaudeSessions;

#[async_trait]
impl IProviderSessions for ClaudeSessions {
    fn normalize_message(&self, raw: Value, session_id: Option<String>) -> Vec<NormalizedMessage> {
        let kind = raw
            .get("type")
            .and_then(|v| v.as_str())
            .map(|t| match t {
                "assistant" => crate::shared::types::MessageKind::Text,
                "tool_use" => crate::shared::types::MessageKind::ToolUse,
                "tool_result" => crate::shared::types::MessageKind::ToolResult,
                _ => crate::shared::types::MessageKind::Text,
            })
            .unwrap_or(crate::shared::types::MessageKind::Text);

        vec![NormalizedMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.unwrap_or_default(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            provider: LlmProvider::Claude,
            kind,
            content: raw.get("content").and_then(|v| v.as_str()).map(String::from),
            ..Default::default()
        }]
    }

    async fn fetch_history(
        &self,
        session_id: String,
        _options: Option<FetchHistoryOptions>,
    ) -> anyhow::Result<FetchHistoryResult> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home directory not found"))?;
        let projects_dir = home.join(".claude").join("projects");

        let mut messages: Vec<NormalizedMessage> = Vec::new();
        if let Ok(mut entries) = tokio::fs::read_dir(&projects_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let jsonl = entry.path().join(format!("{}.jsonl", session_id));
                if let Ok(content) = tokio::fs::read_to_string(&jsonl).await {
                    for line in content.lines() {
                        if let Ok(raw) = serde_json::from_str::<Value>(line.trim()) {
                            messages.extend(self.normalize_message(raw, Some(session_id.clone())));
                        }
                    }
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

pub struct ClaudeSessionSynchronizer;

#[async_trait]
impl IProviderSessionSynchronizer for ClaudeSessionSynchronizer {
    async fn synchronize(&self, since: Option<chrono::DateTime<chrono::Utc>>) -> anyhow::Result<usize> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home not found"))?;
        let projects_dir = home.join(".claude").join("projects");

        let jsonl_files = crate::shared::utils::find_files_recursively_created_after(
            &projects_dir, ".jsonl", since,
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
            "claude",
            &std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .to_string_lossy(),
            None,
            Some(&file_path),
        );

        Ok(Some(session_id.to_string()))
    }
}
