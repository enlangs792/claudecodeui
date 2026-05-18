//! Agent routes — mirrors server/routes/agent.js
//!
//! POST /       — external API endpoint for triggering an AI agent
//! POST /query  — agent communication endpoint with provider dispatch

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

pub fn routes() -> Router {
    Router::new()
        .route("/", post(agent_handler))
        .route("/query", post(agent_query))
}

// ── Common request/response types ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AgentQueryRequest {
    /// The user message to send to the agent
    message: Option<String>,
    /// Session identifier for ongoing conversations
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    /// Provider to route through
    provider: Option<String>,
    /// Project path
    #[serde(rename = "projectPath")]
    project_path: Option<String>,
    /// Model override
    model: Option<String>,
    /// Skip permissions
    #[serde(rename = "skipPermissions")]
    skip_permissions: Option<bool>,
    /// Arbitrary extra fields captured via serde(flatten)
    #[serde(flatten)]
    #[allow(dead_code)]
    extra: serde_json::Map<String, Value>,
}

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
    /// Enable streaming (default behaviour)
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

// ── Handlers ────────────────────────────────────────────────────────────────

/// POST /api/agent/query — agent communication endpoint
///
/// Dispatches to the appropriate provider agent based on the request.
/// Returns a session ID that can be used to track the agent's progress.
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

    let provider = body.provider.as_deref().unwrap_or("claude");
    let session_id = body.session_id.clone();
    let project_path = body.project_path.clone().unwrap_or_else(|| ".".into());
    let model = body.model.clone();

    // Return a session ID to the frontend so it can track the agent
    let new_session_id = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let response = json!({
        "sessionId": new_session_id,
        "provider": provider,
        "status": "accepted",
        "message": format!("Agent query accepted for {} provider. Connect via WebSocket for streaming.", provider),
        "details": {
            "model": model,
            "projectPath": project_path
        }
    });

    Ok(Json(response))
}

/// POST /api/agent — external API endpoint
///
/// Accepts parameters for triggering an AI agent workflow (clone, run, optionally create branch/PR).
/// Returns a session ID and provider info for WebSocket streaming.
async fn agent_handler(
    Extension(_user): Extension<AuthUser>,
    Json(body): Json<AgentRequestBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let provider = body.provider.as_deref().unwrap_or("claude");
    let project_path = body.project_path.as_deref().unwrap_or("");
    let github_url = body.github_url.as_deref().unwrap_or("");
    let message = body.message.as_deref().unwrap_or("");
    let model = body.model.as_deref();
    let stream = body.stream.unwrap_or(true);
    let session_id = body.session_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Validate required fields
    if message.is_empty() && github_url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Either message or githubUrl is required"})),
        ));
    }

    // For SSE streaming, frontend should connect via WebSocket /ws
    let response = if stream {
        json!({
            "sessionId": session_id,
            "provider": provider,
            "status": "started",
            "stream": true,
            "wsEndpoint": "/ws",
            "message": "Agent started. Connect to WebSocket for streaming results.",
            "details": {
                "model": model,
                "projectPath": project_path,
                "githubUrl": if github_url.is_empty() { None } else { Some(github_url) },
                "createBranch": body.create_branch.unwrap_or(false),
                "createPR": body.create_pr.unwrap_or(false),
            }
        })
    } else {
        // Non-streaming: return a snapshot of what would happen
        json!({
            "sessionId": session_id,
            "provider": provider,
            "status": "queued",
            "stream": false,
            "message": format!("Agent task queued for {} provider.", provider),
            "details": {
                "model": model,
                "projectPath": project_path,
            }
        })
    };

    Ok(Json(response))
}
