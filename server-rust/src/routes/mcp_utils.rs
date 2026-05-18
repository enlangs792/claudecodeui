//! MCP utils routes — stub
//!
//! GET /mcp-utils/taskmaster-server — placeholder for MCP taskmaster detection

use axum::{
    Extension,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;

pub fn routes() -> Router {
    Router::new()
        .route("/taskmaster-server", get(taskmaster_server))
}

/// GET /mcp-utils/taskmaster-server — not yet implemented
async fn taskmaster_server(
    Extension(_user): Extension<AuthUser>,
) -> Json<Value> {
    Json(json!({
        "detected": false,
        "configPath": null,
        "message": "MCP utils not yet implemented"
    }))
}
