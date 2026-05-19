//! Codex provider — mirrors server/openai-codex.js

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::shared::providers::*;
use crate::shared::types::*;

// ── Codex Provider ──────────────────────────────────────────────────────────

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

// ── Auth ─────────────────────────────────────────────────────────────────────

pub struct CodexAuth;

#[async_trait]
impl IProviderAuth for CodexAuth {
    async fn get_status(&self) -> anyhow::Result<ProviderAuthStatus> {
        let installed = check_codex_installed();
        let credentials = check_codex_credentials().await;

        Ok(ProviderAuthStatus {
            installed,
            provider: LlmProvider::Codex,
            authenticated: credentials.authenticated,
            email: credentials.email,
            method: credentials.method,
            error: if credentials.authenticated { None } else { credentials.error.or(Some("Not authenticated".into())) },
        })
    }
}

fn check_codex_installed() -> bool {
    std::process::Command::new("codex")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

struct CodexCredentialStatus {
    authenticated: bool,
    email: Option<String>,
    method: Option<String>,
    error: Option<String>,
}

async fn check_codex_credentials() -> CodexCredentialStatus {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return CodexCredentialStatus {
            authenticated: false,
            email: None,
            method: None,
            error: Some("Home directory not found".into()),
        },
    };

    let auth_path = home.join(".codex").join("auth.json");
    let content = match tokio::fs::read_to_string(&auth_path).await {
        Ok(c) => c,
        Err(e) => {
            let msg = if e.kind() == std::io::ErrorKind::NotFound {
                "Codex not configured".to_string()
            } else {
                format!("Failed to read Codex auth: {}", e)
            };
            return CodexCredentialStatus {
                authenticated: false,
                email: None,
                method: None,
                error: Some(msg),
            };
        }
    };

    let auth: Value = serde_json::from_str(&content).unwrap_or_default();

    // Check OAuth tokens (id_token or access_token)
    if let Some(tokens) = auth.get("tokens").and_then(|v| v.as_object()) {
        let id_token = tokens.get("id_token").and_then(|v| v.as_str());
        let access_token = tokens.get("access_token").and_then(|v| v.as_str());

        if let Some(token) = id_token.or(access_token) {
            let email = if id_token.is_some() {
                read_email_from_id_token(id_token.unwrap())
            } else {
                Some("Authenticated".into())
            };

            return CodexCredentialStatus {
                authenticated: true,
                email,
                method: Some("credentials_file".into()),
                error: None,
            };
        }
    }

    // Fallback: OPENAI_API_KEY in auth.json
    if let Some(key) = auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) {
        if !key.is_empty() {
            return CodexCredentialStatus {
                authenticated: true,
                email: Some("API Key Auth".into()),
                method: Some("api_key".into()),
                error: None,
            };
        }
    }

    CodexCredentialStatus {
        authenticated: false,
        email: None,
        method: None,
        error: Some("No valid tokens found".into()),
    }
}

fn read_email_from_id_token(id_token: &str) -> Option<String> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 {
        return Some("Authenticated".into());
    }

    let payload_str = base64url_decode(parts[1]).unwrap_or_default();
    let payload: Value = serde_json::from_str(&payload_str).unwrap_or_default();

    payload.get("email")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("user").and_then(|v| v.as_str()))
        .map(String::from)
        .or(Some("Authenticated".into()))
}

fn base64url_decode(input: &str) -> Result<String, String> {
    let mut padded = input.to_string();
    // Add padding
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    // URL-safe to standard base64
    let standard = padded.replace('-', "+").replace('_', "/");

    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&standard)
        .map_err(|e| e.to_string())?;

    String::from_utf8(bytes).map_err(|e| e.to_string())
}

// ── MCP ──────────────────────────────────────────────────────────────────────

pub struct CodexMcp;

#[async_trait]
impl IProviderMcp for CodexMcp {
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
            McpScope::User => dirs::home_dir().map(|h| h.join(".codex").join("mcp.json")),
            McpScope::Project => None,
            McpScope::Local => None,
        };

        let Some(path) = config_path else {
            return Ok(Vec::new());
        };

        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let config: Value = serde_json::from_str(&content).unwrap_or_default();
                Ok(parse_codex_mcp_config(&config, scope))
            }
            Err(_) => Ok(Vec::new()),
        }
    }

    async fn upsert_server(&self, _input: UpsertProviderMcpServerInput) -> anyhow::Result<ProviderMcpServer> {
        Err(anyhow::anyhow!("MCP upsert not implemented for Codex"))
    }

    async fn remove_server(
        &self,
        name: String,
        scope: Option<McpScope>,
        _workspace_path: Option<String>,
    ) -> anyhow::Result<McpRemovalResult> {
        Ok(McpRemovalResult {
            removed: true,
            provider: LlmProvider::Codex,
            name,
            scope: scope.unwrap_or(McpScope::User),
        })
    }
}

fn parse_codex_mcp_config(config: &Value, scope: McpScope) -> Vec<ProviderMcpServer> {
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
            provider: LlmProvider::Codex,
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

pub struct CodexSkills;

#[async_trait]
impl IProviderSkills for CodexSkills {
    async fn list_skills(&self, _options: Option<ProviderSkillListOptions>) -> anyhow::Result<Vec<ProviderSkill>> {
        Ok(Vec::new())
    }
}

// ── Sessions ─────────────────────────────────────────────────────────────────

pub struct CodexSessions;

#[async_trait]
impl IProviderSessions for CodexSessions {
    fn normalize_message(&self, raw: Value, session_id: Option<String>) -> Vec<NormalizedMessage> {
        let kind = match raw.get("type").and_then(|v| v.as_str()) {
            Some("event_msg") => {
                match raw.get("payload").and_then(|p| p.get("type")).and_then(|v| v.as_str()) {
                    Some("user_msg") | Some("user_text") => MessageKind::Text,
                    Some("assistant_msg") | Some("assistant_text") => MessageKind::Text,
                    Some("tool_use") | Some("tool_use_msg") => MessageKind::ToolUse,
                    Some("tool_result") | Some("tool_result_msg") => MessageKind::ToolResult,
                    _ => MessageKind::Text,
                }
            }
            Some("user") | Some("user_message") => MessageKind::Text,
            Some("assistant") | Some("assistant_message") => MessageKind::Text,
            Some("tool_use") => MessageKind::ToolUse,
            Some("tool_result") => MessageKind::ToolResult,
            _ => MessageKind::Text,
        };

        let content = raw
            .get("content")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| {
                raw.get("payload")
                    .and_then(|p| p.get("content"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });

        vec![NormalizedMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.unwrap_or_default(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            provider: LlmProvider::Codex,
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
        let sessions_dir = home.join(".codex").join("sessions");

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

pub struct CodexSessionSynchronizer;

#[async_trait]
impl IProviderSessionSynchronizer for CodexSessionSynchronizer {
    async fn synchronize(&self, since: Option<chrono::DateTime<chrono::Utc>>) -> anyhow::Result<usize> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home not found"))?;
        let sessions_dir = home.join(".codex").join("sessions");

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

        let project_path = crate::shared::utils::extract_project_path_from_session_file(
            path,
            &std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .to_string_lossy(),
        )
        .await;

        crate::db::repos::sessions::SessionsRepo::upsert(
            session_id,
            "codex",
            &project_path,
            None,
            Some(&file_path),
        );

        Ok(Some(session_id.to_string()))
    }
}
