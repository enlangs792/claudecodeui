//! Agent routes — mirrors server/routes/agent.js
//!
//! POST /       — external API endpoint for triggering an AI agent (Claude, Cursor, Codex, Gemini)
//! POST /query  — stub endpoint for agent communication

use axum::{
    Extension,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::shared::model_constants;

pub fn routes() -> Router {
    Router::new()
        .route("/", post(agent_handler))
        .route("/query", post(agent_query))
}

// ── Common request/response types ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AgentQueryRequest {
    /// The user message to send to the agent
    message: Option<String>,
    /// Session identifier for ongoing conversations
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    /// Provider to route through
    provider: Option<String>,
    /// Arbitrary extra fields captured via serde(flatten)
    #[serde(flatten)]
    #[allow(dead_code)]
    extra: serde_json::Map<String, Value>,
}

/// POST /api/agent — external API endpoint
///
/// Mirrors the Node.js POST /api/agent handler. Accepts parameters for
/// triggering an AI agent workflow (clone, run, optionally create branch/PR).
///
/// The Rust backend returns a stub response — actual agent dispatching will
/// be implemented in a future milestone.
#[derive(Debug, Deserialize)]
struct AgentRequestBody {
    /// GitHub repository URL to clone
    #[serde(rename = "githubUrl")]
    github_url: Option<String>,
    /// Path to existing project OR destination for cloning
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
    /// Task description for the AI agent
    message: Option<String>,
    /// AI provider: "claude", "cursor", "codex", or "gemini"
    provider: Option<String>,
    /// Enable SSE streaming (default: true)
    stream: Option<bool>,
    /// Model override
    model: Option<String>,
    /// GitHub token for private repos / branch/PR creation
    #[serde(rename = "githubToken")]
    github_token: Option<String>,
    /// Custom branch name
    #[serde(rename = "branchName")]
    branch_name: Option<String>,
    /// Session ID to resume
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    /// Create a branch after completion
    #[serde(rename = "createBranch")]
    create_branch: Option<bool>,
    /// Create a PR after completion
    #[serde(rename = "createPR")]
    create_pr: Option<bool>,
    /// Auto-cleanup cloned repos after completion (default: true)
    cleanup: Option<bool>,
    #[serde(flatten)]
    #[allow(dead_code)]
    extra: serde_json::Map<String, Value>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/agent/query — stub for agent communication
async fn agent_query(
    Extension(_user): Extension<AuthUser>,
    Json(body): Json<AgentQueryRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let message = body.message.as_deref().unwrap_or("");

    if message.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "message is required"})),
        ));
    }

    // Stub: echo back the received message with a placeholder response
    let providers = model_constants::providers();
    let provider_info: Vec<Value> = providers.iter().map(|p| {
        json!({
            "id": p.id,
            "name": p.name,
            "models": {
                "options": p.models.options.iter().map(|m| {
                    json!({"value": m.value, "label": m.label})
                }).collect::<Vec<_>>(),
                "default": p.models.default
            }
        })
    }).collect();

    Ok(Json(json!({
        "success": true,
        "response": format!("Received: {}. Agent processing is not yet implemented.", message),
        "sessionId": body.session_id,
        "provider": body.provider,
        "availableProviders": provider_info,
    })))
}

/// POST /api/agent — external API endpoint (stub)
///
/// Validates inputs matching the Node.js shape and returns a stub response.
async fn agent_handler(
    Extension(_user): Extension<AuthUser>,
    Json(body): Json<AgentRequestBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let message = body.message.as_deref().unwrap_or("");

    // Validate: either githubUrl or projectPath must be provided
    if body.github_url.is_none() && body.project_path.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Either githubUrl or projectPath is required"})),
        ));
    }

    // Validate: message must be non-empty
    if message.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "message is required"})),
        ));
    }

    // Validate: provider must be one of the known values
    let provider = body.provider.as_deref().unwrap_or("claude");
    if !["claude", "cursor", "codex", "gemini"].contains(&provider) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "provider must be \"claude\", \"cursor\", \"codex\", or \"gemini\""})),
        ));
    }

    // Determine whether to create branch or PR
    let create_branch = body.branch_name.is_some() || body.create_branch.unwrap_or(false);
    let create_pr = body.create_pr.unwrap_or(false);

    // Determine the project path for the response
    let final_project_path = body.project_path.clone().unwrap_or_else(|| {
        body.github_url.clone().map(|url| {
            format!("/tmp/claude-external-projects/{}", simple_hash(&url))
        }).unwrap_or_default()
    });

    // Build a stub response (non-streaming)
    Ok(Json(json!({
        "success": true,
        "sessionId": body.session_id,
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": format!("Agent execution is not yet implemented in the Rust backend.\n\nReceived message: {}", message)
                    }
                ]
            }
        ],
        "tokens": {
            "inputTokens": 0,
            "outputTokens": 0,
            "cacheReadTokens": 0,
            "cacheCreationTokens": 0,
            "totalTokens": 0
        },
        "projectPath": final_project_path,
        "branch": if create_branch {
            Some(json!({"name": body.branch_name.as_deref().unwrap_or("auto-generated"), "url": null}))
        } else { None },
        "pullRequest": if create_pr {
            Some(json!({"number": 0, "url": null}))
        } else { None }
    })))
}

/// Simple non-cryptographic hash for generating temp directory names
fn simple_hash(input: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}
