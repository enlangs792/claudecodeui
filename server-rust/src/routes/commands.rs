//! Commands route — mirrors server/modules/commands/commands.routes.ts
//!
//! GET /api/commands — returns available slash commands sourced from
//! provider model constants.

use axum::{
    Extension,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::shared::model_constants;

pub fn routes() -> Router {
    Router::new()
        .route("/", get(list_commands))
}

/// GET /api/commands — return the list of available slash commands
async fn list_commands(
    Extension(_user): Extension<AuthUser>,
) -> Json<Value> {
    let providers = model_constants::providers();

    let commands: Vec<Value> = providers.iter().map(|p| {
        json!({
            "id": p.id,
            "name": p.name,
            "models": {
                "options": p.models.options.iter().map(|m| {
                    json!({
                        "value": m.value,
                        "label": m.label
                    })
                }).collect::<Vec<_>>(),
                "default": p.models.default
            }
        })
    }).collect();

    Json(json!({
        "commands": commands
    }))
}
