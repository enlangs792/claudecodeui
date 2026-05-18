//! Settings routes — mirrors server/modules/settings/settings.routes.ts
//!
//! GET  /api/settings — returns app settings from app_config
//! PUT  /api/settings — updates app settings

use axum::{
    Extension,
    response::Json,
    routing::{get, put},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::auth::middleware::AuthUser;
use crate::db::repos::app_config::AppConfigRepo;

pub fn routes() -> Router {
    Router::new()
        .route("/", get(get_settings).put(update_settings))
}

/// GET /api/settings — return all stored app config key-value pairs
async fn get_settings(
    Extension(_user): Extension<AuthUser>,
) -> Json<Value> {
    // AppConfigRepo only provides get(key) — for a full listing we return
    // a curated set of known config keys.
    let known_keys = [
        "jwt_secret",
        "theme",
        "fontSize",
        "language",
        "autoSave",
        "tabSize",
    ];

    let mut settings = HashMap::new();
    for key in &known_keys {
        if let Some(value) = AppConfigRepo::get(key) {
            settings.insert(key.to_string(), value);
        }
    }

    Json(json!({
        "settings": settings
    }))
}

#[derive(Debug, Deserialize)]
struct UpdateSettingsBody {
    #[serde(flatten)]
    settings: HashMap<String, String>,
}

/// PUT /api/settings — upsert key-value pairs into app_config
async fn update_settings(
    Extension(_user): Extension<AuthUser>,
    Json(body): Json<UpdateSettingsBody>,
) -> Json<Value> {
    for (key, value) in &body.settings {
        AppConfigRepo::set(key, value);
    }

    Json(json!({
        "success": true,
        "message": "Settings updated successfully"
    }))
}
