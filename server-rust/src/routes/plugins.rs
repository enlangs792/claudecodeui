//! Plugins routes — stub
//!
//! GET /plugins — list available plugins

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
        .route("/", get(list_plugins))
}

/// GET /plugins — return empty plugin list (stub)
async fn list_plugins(
    Extension(_user): Extension<AuthUser>,
) -> Json<Value> {
    Json(json!({
        "plugins": []
    }))
}
