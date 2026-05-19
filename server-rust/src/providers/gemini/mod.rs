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
        let installed = check_gemini_installed();

        if !installed {
            return Ok(ProviderAuthStatus {
                installed: false,
                provider: LlmProvider::Gemini,
                authenticated: false,
                email: None,
                method: None,
                error: Some("Gemini CLI is not installed".into()),
            });
        }

        let credentials = check_gemini_credentials().await;

        Ok(ProviderAuthStatus {
            installed,
            provider: LlmProvider::Gemini,
            authenticated: credentials.authenticated,
            email: credentials.email,
            method: credentials.method,
            error: if credentials.authenticated { None } else { credentials.error.or(Some("Not authenticated".into())) },
        })
    }
}

fn get_gemini_cli_home() -> std::path::PathBuf {
    if let Ok(val) = std::env::var("GEMINI_CLI_HOME") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return std::path::PathBuf::from(trimmed);
        }
    }
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn check_gemini_installed() -> bool {
    let cli_path = std::env::var("GEMINI_PATH").unwrap_or_else(|_| "gemini".into());
    std::process::Command::new(&cli_path)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

struct GeminiCredentialStatus {
    authenticated: bool,
    email: Option<String>,
    method: Option<String>,
    error: Option<String>,
}

async fn check_gemini_credentials() -> GeminiCredentialStatus {
    // 1. Check GEMINI_API_KEY in process env
    if let Ok(val) = std::env::var("GEMINI_API_KEY") {
        if !val.trim().is_empty() {
            return GeminiCredentialStatus {
                authenticated: true,
                email: Some("API Key Auth".into()),
                method: Some("api_key".into()),
                error: None,
            };
        }
    }

    // 2. Read ~/.gemini/.env and ~/.env for user-level auth env
    let user_env = load_gemini_user_env().await;

    if let Some(val) = user_env.get("GEMINI_API_KEY") {
        if !val.is_empty() {
            return GeminiCredentialStatus {
                authenticated: true,
                email: Some("API Key Auth".into()),
                method: Some("api_key".into()),
                error: None,
            };
        }
    }

    // 3. Read selected auth type from settings.json
    let selected_type = read_gemini_selected_auth_type().await;

    // 4. Handle Vertex AI
    if selected_type.as_deref() == Some("vertex-ai") {
        let has_google_api_key = std::env::var("GOOGLE_API_KEY").ok()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
            || user_env.get("GOOGLE_API_KEY").map(|v| !v.is_empty()).unwrap_or(false);

        let has_project = std::env::var("GOOGLE_CLOUD_PROJECT").ok()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
            || std::env::var("GOOGLE_CLOUD_PROJECT_ID").ok()
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
            || user_env.get("GOOGLE_CLOUD_PROJECT").map(|v| !v.is_empty()).unwrap_or(false)
            || user_env.get("GOOGLE_CLOUD_PROJECT_ID").map(|v| !v.is_empty()).unwrap_or(false);

        let has_location = std::env::var("GOOGLE_CLOUD_LOCATION").ok()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
            || user_env.get("GOOGLE_CLOUD_LOCATION").map(|v| !v.is_empty()).unwrap_or(false);

        let has_service_account = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
            || user_env.get("GOOGLE_APPLICATION_CREDENTIALS").map(|v| !v.is_empty()).unwrap_or(false);

        if has_google_api_key || has_service_account || (has_project && has_location) {
            return GeminiCredentialStatus {
                authenticated: true,
                email: Some("Vertex AI Auth".into()),
                method: Some("vertex_ai".into()),
                error: None,
            };
        }

        return GeminiCredentialStatus {
            authenticated: false,
            email: None,
            method: Some("vertex_ai".into()),
            error: Some("Gemini is set to Vertex AI, but required env vars are missing".into()),
        };
    }

    // 5. Check OAuth credentials
    let oauth_result = check_gemini_oauth_credentials(&user_env, &selected_type).await;
    if oauth_result.is_some() {
        return oauth_result.unwrap();
    }

    // 6. Handle explicit auth type selection with missing credentials
    if selected_type.as_deref() == Some("gemini-api-key") {
        return GeminiCredentialStatus {
            authenticated: false,
            email: None,
            method: Some("api_key".into()),
            error: Some("Gemini is set to \"Use Gemini API key\", but GEMINI_API_KEY is unavailable".into()),
        };
    }

    if selected_type.as_deref() == Some("oauth-personal") {
        return GeminiCredentialStatus {
            authenticated: false,
            email: None,
            method: Some("credentials_file".into()),
            error: Some("Gemini is set to Google sign-in, but no cached OAuth credentials were found".into()),
        };
    }

    GeminiCredentialStatus {
        authenticated: false,
        email: None,
        method: None,
        error: Some("Gemini CLI not configured".into()),
    }
}

async fn load_gemini_user_env() -> std::collections::HashMap<String, String> {
    let gemini_home = get_gemini_cli_home();
    let env_candidates = vec![
        gemini_home.join(".gemini").join(".env"),
        gemini_home.join(".env"),
    ];

    for env_path in env_candidates {
        if let Ok(content) = tokio::fs::read_to_string(&env_path).await {
            return parse_dotenv_content(&content);
        }
    }

    std::collections::HashMap::new()
}

fn parse_dotenv_content(content: &str) -> std::collections::HashMap<String, String> {
    let mut parsed = std::collections::HashMap::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let normalized = line.strip_prefix("export ").unwrap_or(line);
        let separator_index = normalized.find('=');
        let Some(eq_pos) = separator_index else {
            continue;
        };

        if eq_pos == 0 {
            continue;
        }

        let key = normalized[..eq_pos].trim().to_string();
        if key.is_empty() {
            continue;
        }

        let mut value = normalized[eq_pos + 1..].trim().to_string();

        // Strip quotes
        if (value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\''))
        {
            value = value[1..value.len() - 1].to_string();
        } else {
            // Strip inline comments
            if let Some(comment_pos) = value.find(" #") {
                value = value[..comment_pos].trim().to_string();
            }
        }

        parsed.insert(key, value);
    }

    parsed
}

async fn read_gemini_selected_auth_type() -> Option<String> {
    let gemini_home = get_gemini_cli_home();
    let settings_path = gemini_home.join(".gemini").join("settings.json");

    let content = tokio::fs::read_to_string(&settings_path).await.ok()?;
    let settings: Value = serde_json::from_str(&content).ok()?;

    settings
        .get("security")
        .and_then(|s| s.get("auth"))
        .and_then(|a| a.get("selectedType"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

async fn check_gemini_oauth_credentials(
    user_env: &std::collections::HashMap<String, String>,
    _selected_type: &Option<String>,
) -> Option<GeminiCredentialStatus> {
    let gemini_home = get_gemini_cli_home();
    let creds_path = gemini_home.join(".gemini").join("oauth_creds.json");

    let content = tokio::fs::read_to_string(&creds_path).await.ok()?;
    let creds: Value = serde_json::from_str(&content).ok()?;

    let access_token = creds.get("access_token").and_then(|v| v.as_str());
    if access_token.is_none() || access_token == Some("") {
        return Some(GeminiCredentialStatus {
            authenticated: false,
            email: None,
            method: None,
            error: Some("No valid tokens found in oauth_creds".into()),
        });
    }

    let access_token = access_token.unwrap();
    let refresh_token = creds.get("refresh_token").and_then(|v| v.as_str());

    // If refresh_token exists, Gemini CLI can refresh — treat as authenticated
    if refresh_token.is_some() {
        let email = get_active_gemini_account_email().await;
        return Some(GeminiCredentialStatus {
            authenticated: true,
            email: Some(email.unwrap_or_else(|| "OAuth Session".into())),
            method: Some("credentials_file".into()),
            error: None,
        });
    }

    // Access token present, no refresh token — trust it's valid
    // (network validation skipped to avoid reqwest dependency)
    Some(GeminiCredentialStatus {
        authenticated: true,
        email: Some("OAuth Session".into()),
        method: Some("credentials_file".into()),
        error: None,
    })
}

async fn get_active_gemini_account_email() -> Option<String> {
    let gemini_home = get_gemini_cli_home();
    let acc_path = gemini_home.join(".gemini").join("google_accounts.json");

    let content = tokio::fs::read_to_string(&acc_path).await.ok()?;
    let accounts: Value = serde_json::from_str(&content).ok()?;
    accounts.get("active").and_then(|v| v.as_str()).map(String::from)
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
        let tmp_dir = home.join(".gemini").join("tmp");

        let mut jsonl_files = crate::shared::utils::find_files_recursively_created_after(
            &tmp_dir, ".jsonl", since,
        )
        .await;

        // Also scan .json files (Gemini uses both .jsonl and .json)
        let json_files = crate::shared::utils::find_files_recursively_created_after(
            &tmp_dir, ".json", since,
        )
        .await;
        jsonl_files.extend(json_files);

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
            "gemini",
            &project_path,
            None,
            Some(&file_path),
        );

        Ok(Some(session_id.to_string()))
    }
}
