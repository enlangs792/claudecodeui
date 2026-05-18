//! Shared utilities — mirrors server/shared/utils.ts
//!
//! Core helpers for path validation, message creation, config parsing,
//! skill file discovery, session sync, and JSONL parsing.

use crate::shared::types::{
    ApiSuccess, AnyRecord, AppErrorOptions, NormalizedMessage, WorkspacePathValidationResult, LlmProvider, MessageKind,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

// ── HTTP Handler Utilities ───────────────────────────────────────────────────

/// Return a standard API success envelope
pub fn api_success<T: serde::Serialize>(data: T) -> ApiSuccess<T> {
    ApiSuccess::new(data)
}

// ── Error Types ──────────────────────────────────────────────────────────────

/// Application error with HTTP status and machine-readable code
#[derive(Debug, thiserror::Error)]
pub struct AppError {
    pub message: String,
    pub code: String,
    pub status_code: u16,
    pub details: Option<Value>,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl AppError {
    pub fn new(message: impl Into<String>, options: AppErrorOptions) -> Self {
        Self {
            message: message.into(),
            code: options.code.unwrap_or_else(|| "INTERNAL_ERROR".into()),
            status_code: options.status_code.unwrap_or(500),
            details: options.details,
        }
    }
}

// ── Workspace Path Validation ────────────────────────────────────────────────

/// Root directory for all workspace paths
pub fn workspaces_root() -> PathBuf {
    std::env::var("WORKSPACES_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
}

/// System-critical paths that must never be used as workspace roots
pub const FORBIDDEN_WORKSPACE_PATHS: &[&str] = &[
    "/", "/etc", "/bin", "/sbin", "/usr", "/dev", "/proc", "/sys",
    "/var", "/boot", "/root", "/lib", "/lib64", "/opt", "/tmp", "/run",
];

/// Validate that a user-supplied path is safe to use as a workspace
pub async fn validate_workspace_path(requested_path: &str) -> WorkspacePathValidationResult {
    let normalized = normalize_project_path(requested_path);
    if normalized.is_empty() {
        return WorkspacePathValidationResult {
            valid: false,
            resolved_path: None,
            error: Some("Workspace path is required".into()),
        };
    }

    let absolute = std::path::absolute(&normalized).unwrap_or_else(|_| PathBuf::from(&normalized));
    let abs_path = normalize_project_path(&absolute.to_string_lossy());

    // Block system-critical directories
    for forbidden in FORBIDDEN_WORKSPACE_PATHS {
        let nf = normalize_project_path(forbidden);
        if abs_path == nf || abs_path.starts_with(&format!("{}/", nf)) {
            // Allow /var/tmp and /var/folders
            if nf == "/var" && (abs_path.starts_with("/var/tmp") || abs_path.starts_with("/var/folders")) {
                continue;
            }
            return WorkspacePathValidationResult {
                valid: false,
                resolved_path: None,
                error: Some(format!("Cannot create workspace in system directory: {}", forbidden)),
            };
        }
    }

    // Resolve symlinks if file exists
    let resolved = match tokio::fs::canonicalize(&absolute).await {
        Ok(p) => normalize_project_path(&p.to_string_lossy()),
        Err(_) => abs_path.clone(),
    };

    let workspace_root = workspaces_root();
    let root_resolved = tokio::fs::canonicalize(&workspace_root)
        .await
        .map(|p| normalize_project_path(&p.to_string_lossy()))
        .unwrap_or_else(|_| normalize_project_path(&workspace_root.to_string_lossy()));

    if !resolved.starts_with(&format!("{}/", root_resolved)) && resolved != root_resolved {
        return WorkspacePathValidationResult {
            valid: false,
            resolved_path: None,
            error: Some(format!(
                "Workspace path must be within the allowed workspace root: {}",
                workspace_root.display()
            )),
        };
    }

    WorkspacePathValidationResult {
        valid: true,
        resolved_path: Some(resolved),
        error: None,
    }
}

/// Normalize a project path for stable DB keys
pub fn normalize_project_path(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Strip Windows long-path prefixes
    let without_prefix = if trimmed.starts_with("\\\\?\\UNC\\") {
        format!("\\\\{}", &trimmed["\\\\?\\UNC\\".len()..])
    } else if trimmed.starts_with("\\\\?\\") {
        trimmed["\\\\?\\".len()..].to_string()
    } else {
        trimmed.to_string()
    };

    // Use POSIX normalization
    let path = Path::new(&without_prefix);
    let normalized = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");

    if normalized.is_empty() {
        return String::new();
    }

    // Strip trailing separator except for root
    if normalized != "/" {
        normalized.trim_end_matches('/').to_string()
    } else {
        normalized
    }
}

// ── Normalized Provider Message Utilities ────────────────────────────────────

/// Generate a stable unique ID for normalized messages
pub fn generate_message_id(prefix: &str) -> String {
    format!("{}_{}", prefix, uuid::Uuid::new_v4())
}

/// Create a normalized provider message with shared envelope fields
pub fn create_normalized_message(
    kind: MessageKind,
    provider: LlmProvider,
    session_id: Option<String>,
    extra: serde_json::Map<String, Value>,
) -> NormalizedMessage {
    NormalizedMessage {
        id: generate_message_id(match kind {
            MessageKind::Text => "msg",
            MessageKind::ToolUse => "tool",
            MessageKind::ToolResult => "toolr",
            MessageKind::Thinking => "think",
            MessageKind::StreamDelta => "delta",
            MessageKind::StreamEnd => "end",
            MessageKind::Error => "err",
            MessageKind::Complete => "done",
            MessageKind::Status => "status",
            MessageKind::PermissionRequest => "perm",
            MessageKind::PermissionCancelled => "permc",
            MessageKind::SessionCreated => "sess",
            MessageKind::InteractivePrompt => "prompt",
            MessageKind::TaskNotification => "task",
        }),
        session_id: session_id.unwrap_or_default(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        provider,
        kind,
        role: None,
        content: None,
        display_text: None,
        command_name: None,
        command_message: None,
        command_args: None,
        is_local_command: None,
        is_local_command_stdout: None,
        is_compact_summary: None,
        images: None,
        tool_name: None,
        tool_input: None,
        tool_id: None,
        tool_result: None,
        is_error: None,
        text: None,
        tokens: None,
        can_interrupt: None,
        request_id: None,
        input: None,
        context: None,
        reason: None,
        new_session_id: None,
        status: None,
        summary: None,
        token_budget: None,
        subagent_tools: None,
        tool_use_result: None,
        sequence: None,
        rowid: None,
        extra,
    }
}

// ── Config Parsing ────────────────────────────────────────────────────────────

/// Safely read a plain object from unknown JSON value
pub fn read_object_record(value: &Value) -> Option<serde_json::Map<String, Value>> {
    match value {
        Value::Object(map) if !map.is_empty() => Some(map.clone()),
        _ => None,
    }
}

/// Read optional trimmed string from unknown input
pub fn read_optional_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    }
}

/// Read optional string array from unknown input
pub fn read_string_array(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Array(arr) => {
            let strings: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if strings.is_empty() {
                None
            } else {
                Some(strings)
            }
        }
        _ => None,
    }
}

/// Read an optional string-to-string map from unknown input
pub fn read_string_record(value: &Value) -> Option<serde_json::Map<String, Value>> {
    let map = read_object_record(value)?;
    let filtered: serde_json::Map<String, Value> = map
        .into_iter()
        .filter(|(_, v)| v.is_string())
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

// ── JSON Config File I/O ─────────────────────────────────────────────────────

/// Read a JSON config file, returning empty object if missing
pub async fn read_json_config(file_path: &Path) -> anyhow::Result<serde_json::Map<String, Value>> {
    match tokio::fs::read_to_string(file_path).await {
        Ok(content) => {
            let parsed: Value = serde_json::from_str(&content)?;
            Ok(read_object_record(&parsed).unwrap_or_default())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e.into()),
    }
}

/// Write a JSON config file with human-readable formatting
pub async fn write_json_config(file_path: &Path, data: &Value) -> anyhow::Result<()> {
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = format!("{}\n", serde_json::to_string_pretty(data)?);
    tokio::fs::write(file_path, content).await?;
    Ok(())
}

// ── Skill File Discovery ─────────────────────────────────────────────────────

/// Find SKILL.md files under a provider skill root
pub async fn find_provider_skill_markdown_files(
    root_dir: &Path,
    recursive: bool,
) -> Vec<String> {
    let mut files = Vec::new();

    if recursive {
        collect_skill_files_recursive(root_dir, &mut files).await;
    } else {
        collect_skill_files_direct(root_dir, &mut files).await;
    }

    files.sort();
    files
}

async fn collect_skill_files_direct(root: &Path, files: &mut Vec<String>) {
    let mut entries = match tokio::fs::read_dir(root).await {
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
        let skill_path = entry.path().join("SKILL.md");
        if tokio::fs::metadata(&skill_path).await.map(|m| m.is_file()).unwrap_or(false) {
            files.push(skill_path.to_string_lossy().into_owned());
        }
    }
}

async fn collect_skill_files_recursive(root: &Path, files: &mut Vec<String>) {
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(e) => e,
        Err(_) => return,
    };

    // Check for SKILL.md in current directory
    let skill_path = root.join("SKILL.md");
    if tokio::fs::metadata(&skill_path).await.map(|m| m.is_file()).unwrap_or(false) {
        files.push(skill_path.to_string_lossy().into_owned());
    }

    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.file_type().await.map(|ft| ft.is_dir()).unwrap_or(false) {
            Box::pin(collect_skill_files_recursive(&entry.path(), files)).await;
        }
    }
}

// ── Session Synchronizer Helpers ─────────────────────────────────────────────

/// Normalize a session name for UI display
pub fn normalize_session_name(raw: Option<&str>, fallback: &str) -> String {
    let normalized = raw.unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        let mut result = normalized;
        result.truncate(120);
        result
    }
}

/// Find files matching an extension, optionally filtered by creation time
pub async fn find_files_recursively_created_after(
    root_dir: &Path,
    extension: &str,
    last_scan_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<String> {
    let mut files = Vec::new();
    collect_files_by_ext(root_dir, extension, last_scan_at, &mut files).await;
    files
}

async fn collect_files_by_ext(
    dir: &Path,
    ext: &str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    files: &mut Vec<String>,
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
        let path = entry.path();

        if ft.is_dir() {
            Box::pin(collect_files_by_ext(&path, ext, since, files)).await;
        } else if ft.is_file() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(ext) {
                continue;
            }
            if let Some(since) = since {
                if let Ok(meta) = tokio::fs::metadata(&path).await {
                    if let Ok(created) = meta.created() {
                        let created_dt: chrono::DateTime<chrono::Utc> = created.into();
                        if created_dt <= since {
                            continue;
                        }
                    }
                }
            }
            files.push(path.to_string_lossy().into_owned());
        }
    }
}

/// Read file timestamps (created_at, updated_at)
pub async fn read_file_timestamps(file_path: &Path) -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    if let Ok(meta) = tokio::fs::metadata(file_path).await {
        if let Ok(created) = meta.created() {
            let created_dt: chrono::DateTime<chrono::Utc> = created.into();
            result.insert("createdAt".into(), created_dt.to_rfc3339());
        }
        if let Ok(modified) = meta.modified() {
            let modified_dt: chrono::DateTime<chrono::Utc> = modified.into();
            result.insert("updatedAt".into(), modified_dt.to_rfc3339());
        }
    }
    result
}

// ── JSONL Parsing Helpers ────────────────────────────────────────────────────

/// Build a first-seen key/value lookup map from a JSONL file
pub async fn build_lookup_map(
    file_path: &Path,
    key_field: &str,
    value_field: &str,
) -> std::collections::HashMap<String, String> {
    let content = match tokio::fs::read_to_string(file_path).await {
        Ok(c) => c,
        Err(_) => return Default::default(),
    };

    let mut map = std::collections::HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
            if let (Some(key), Some(value)) = (parsed.get(key_field), parsed.get(value_field)) {
                if let (Some(k), Some(v)) = (key.as_str(), value.as_str()) {
                    map.entry(k.to_string()).or_insert_with(|| v.to_string());
                }
            }
        }
    }
    map
}

// ── Incoming Payload Parsing ─────────────────────────────────────────────────

/// Parse an incoming websocket payload into a JSON object
pub fn parse_incoming_json_object(payload: &[u8]) -> Option<serde_json::Map<String, Value>> {
    let text = std::str::from_utf8(payload).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed: Value = serde_json::from_str(trimmed).ok()?;
    read_object_record(&parsed)
}
