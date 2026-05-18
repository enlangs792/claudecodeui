//! Shared types — mirrors server/shared/types.ts
//!
//! Every TypeScript type/interface in the original server has a corresponding
//! Rust struct/enum/type here with serde Serialize/Deserialize for JSON compat.

use serde::{Deserialize, Serialize};

// ── HTTP Response Shapes ────────────────────────────────────────────────────

/// Canonical success envelope (ApiSuccessShape<TData>)
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiSuccess<T: Serialize> {
    pub success: bool,
    pub data: T,
}

impl<T: Serialize> ApiSuccess<T> {
    pub fn new(data: T) -> Self {
        Self {
            success: true,
            data,
        }
    }
}

/// Generic JSON record — use only after runtime shape checks
pub type AnyRecord = serde_json::Map<String, serde_json::Value>;

// ── WebSocket Transport Types ───────────────────────────────────────────────

/// Provider identifiers (LLMProvider)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    #[default]
    Claude,
    Codex,
    Gemini,
    Cursor,
}

impl LlmProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Cursor => "cursor",
        }
    }
}

// ── Message model ────────────────────────────────────────────────────────────

/// Message/event kinds (MessageKind)
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    #[default]
    Text,
    ToolUse,
    ToolResult,
    Thinking,
    StreamDelta,
    StreamEnd,
    Error,
    Complete,
    Status,
    PermissionRequest,
    PermissionCancelled,
    SessionCreated,
    InteractivePrompt,
    TaskNotification,
}

/// Provider-neutral message envelope (NormalizedMessage)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NormalizedMessage {
    pub id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub timestamp: String,
    pub provider: LlmProvider,
    pub kind: MessageKind,
    pub role: Option<MessageRole>,
    pub content: Option<String>,
    #[serde(rename = "displayText", skip_serializing_if = "Option::is_none")]
    pub display_text: Option<String>,
    #[serde(rename = "commandName", skip_serializing_if = "Option::is_none")]
    pub command_name: Option<String>,
    #[serde(rename = "commandMessage", skip_serializing_if = "Option::is_none")]
    pub command_message: Option<String>,
    #[serde(rename = "commandArgs", skip_serializing_if = "Option::is_none")]
    pub command_args: Option<String>,
    #[serde(rename = "isLocalCommand", skip_serializing_if = "Option::is_none")]
    pub is_local_command: Option<bool>,
    #[serde(rename = "isLocalCommandStdout", skip_serializing_if = "Option::is_none")]
    pub is_local_command_stdout: Option<bool>,
    #[serde(rename = "isCompactSummary", skip_serializing_if = "Option::is_none")]
    pub is_compact_summary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<serde_json::Value>,
    #[serde(rename = "toolName", skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(rename = "toolInput", skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    #[serde(rename = "toolId", skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(rename = "toolResult", skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResult>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(rename = "canInterrupt", skip_serializing_if = "Option::is_none")]
    pub can_interrupt: Option<bool>,
    #[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "newSessionId", skip_serializing_if = "Option::is_none")]
    pub new_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "tokenBudget", skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<serde_json::Value>,
    #[serde(rename = "subagentTools", skip_serializing_if = "Option::is_none")]
    pub subagent_tools: Option<serde_json::Value>,
    #[serde(rename = "toolUseResult", skip_serializing_if = "Option::is_none")]
    pub tool_use_result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rowid: Option<i64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(rename = "toolUseResult", skip_serializing_if = "Option::is_none")]
    pub tool_use_result: Option<serde_json::Value>,
}

// ── History fetch types ──────────────────────────────────────────────────────

/// Options for fetching historical messages (FetchHistoryOptions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchHistoryOptions {
    #[serde(rename = "projectPath", skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

/// Standardized history response (FetchHistoryResult)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchHistoryResult {
    pub messages: Vec<NormalizedMessage>,
    pub total: u32,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
    pub offset: u32,
    pub limit: Option<u32>,
    #[serde(rename = "tokenUsage", skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<serde_json::Value>,
}

// ── Provider Skill Types ─────────────────────────────────────────────────────

/// Where a skill originates (ProviderSkillScope)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderSkillScope {
    #[default]
    User,
    Project,
    Plugin,
    Repo,
    Admin,
    System,
}

/// Input for listing skills (ProviderSkillListOptions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSkillListOptions {
    #[serde(rename = "workspacePath", skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

/// Normalized skill record (ProviderSkill)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSkill {
    pub provider: LlmProvider,
    pub name: String,
    pub description: String,
    pub command: String,
    pub scope: ProviderSkillScope,
    #[serde(rename = "sourcePath")]
    pub source_path: String,
    #[serde(rename = "pluginName", skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    #[serde(rename = "pluginId", skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
}

/// Skill source descriptor (ProviderSkillSource)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSkillSource {
    pub scope: ProviderSkillScope,
    #[serde(rename = "rootDir")]
    pub root_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recursive: Option<bool>,
    #[serde(rename = "commandPrefix", skip_serializing_if = "Option::is_none")]
    pub command_prefix: Option<String>,
    #[serde(rename = "pluginName", skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    #[serde(rename = "pluginId", skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
}

// ── Error Types ──────────────────────────────────────────────────────────────

/// Application error options (AppErrorOptions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppErrorOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(rename = "statusCode", skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

// ── MCP Types ────────────────────────────────────────────────────────────────

/// MCP configuration scope (McpScope)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpScope {
    #[default]
    User,
    Local,
    Project,
}

/// MCP transport protocol (McpTransport)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    #[default]
    Stdio,
    Http,
    Sse,
}

/// Normalized MCP server definition (ProviderMcpServer)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderMcpServer {
    pub provider: LlmProvider,
    pub name: String,
    pub scope: McpScope,
    pub transport: McpTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(rename = "envVars", skip_serializing_if = "Option::is_none")]
    pub env_vars: Option<Vec<String>>,
    #[serde(rename = "bearerTokenEnvVar", skip_serializing_if = "Option::is_none")]
    pub bearer_token_env_var: Option<String>,
    #[serde(rename = "envHttpHeaders", skip_serializing_if = "Option::is_none")]
    pub env_http_headers: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Upsert input for MCP servers (UpsertProviderMcpServerInput)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertProviderMcpServerInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<McpScope>,
    pub transport: McpTransport,
    #[serde(rename = "workspacePath", skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(rename = "envVars", skip_serializing_if = "Option::is_none")]
    pub env_vars: Option<Vec<String>>,
    #[serde(rename = "bearerTokenEnvVar", skip_serializing_if = "Option::is_none")]
    pub bearer_token_env_var: Option<String>,
    #[serde(rename = "envHttpHeaders", skip_serializing_if = "Option::is_none")]
    pub env_http_headers: Option<serde_json::Map<String, serde_json::Value>>,
}

// ── Provider Auth Types ──────────────────────────────────────────────────────

/// Auth status result (ProviderAuthStatus)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAuthStatus {
    pub installed: bool,
    pub provider: LlmProvider,
    pub authenticated: bool,
    pub email: Option<String>,
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Database Credential Types ────────────────────────────────────────────────

/// Safe credential view (CredentialPublicRow)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialPublicRow {
    pub id: i64,
    pub credential_name: String,
    pub credential_type: String,
    pub description: Option<String>,
    pub created_at: String,
    pub is_active: i32,
}

/// Result of creating a credential (CreateCredentialResult)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCredentialResult {
    pub id: i64,
    #[serde(rename = "credentialName")]
    pub credential_name: String,
    #[serde(rename = "credentialType")]
    pub credential_type: String,
}

// ── Project Persistence Types ────────────────────────────────────────────────

/// Project database row (ProjectRepositoryRow)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRepositoryRow {
    pub project_id: String,
    pub project_path: String,
    pub custom_project_name: Option<String>,
    #[serde(rename = "isStarred")]
    pub is_starred: i32,
    #[serde(rename = "isArchived")]
    pub is_archived: i32,
}

/// Outcome of project path creation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreateProjectPathOutcome {
    Created,
    ReactivatedArchived,
    ActiveConflict,
}

/// Structured project creation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectPathResult {
    pub outcome: CreateProjectPathOutcome,
    pub project: Option<ProjectRepositoryRow>,
}

/// Workspace path validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePathValidationResult {
    pub valid: bool,
    #[serde(rename = "resolvedPath", skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
