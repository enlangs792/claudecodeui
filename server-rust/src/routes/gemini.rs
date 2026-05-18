//! Gemini routes — mirrors Gemini session management
//!
//! DELETE /gemini/sessions/:session_id — delete a session

use axum::{
    extract::Path,
    Extension,
    response::Json,
    routing::delete,
    Router,
};
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;

pub fn routes() -> Router {
    Router::new()
        .route("/sessions/{session_id}", delete(delete_session))
}

/// DELETE /gemini/sessions/:session_id — log deletion and confirm
async fn delete_session(
    Extension(_user): Extension<AuthUser>,
    Path(session_id): Path<String>,
) -> Json<Value> {
    tracing::info!("Gemini session deleted: {}", session_id);
    Json(json!({
        "success": true
    }))
}
