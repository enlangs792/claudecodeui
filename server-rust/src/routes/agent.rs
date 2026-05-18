//! Agent routes — mirrors server/modules/agent/agent.routes.ts
//!
//! POST /api/agent/query — stub endpoint for agent communication.
//! Accepts a JSON body and returns a basic response.

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
        .route("/query", post(agent_query))
}

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
    Ok(Json(json!({
        "success": true,
        "response": format!("Received: {}. Agent processing is not yet implemented.", message),
        "sessionId": body.session_id,
        "provider": body.provider,
    })))
}
