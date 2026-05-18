//! Cursor routes — mirrors cursor-related config endpoints
//!
//! GET /cursor/config — read ~/.cursor/cli-config.json

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
        .route("/config", get(get_config))
}

/// GET /cursor/config — return parsed ~/.cursor/cli-config.json or default
async fn get_config(
    Extension(_user): Extension<AuthUser>,
) -> Json<Value> {
    let config_path = dirs::home_dir()
        .map(|p| p.join(".cursor").join("cli-config.json"));

    match config_path {
        Some(path) if path.exists() => {
            match tokio::fs::read_to_string(&path).await {
                Ok(content) => {
                    match serde_json::from_str::<Value>(&content) {
                        Ok(parsed) => Json(parsed),
                        Err(e) => {
                            tracing::warn!("Failed to parse cursor config: {}", e);
                            Json(json!({"version": 1, "models": {}}))
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to read cursor config: {}", e);
                    Json(json!({"version": 1, "models": {}}))
                }
            }
        }
        _ => Json(json!({"version": 1, "models": {}})),
    }
}
