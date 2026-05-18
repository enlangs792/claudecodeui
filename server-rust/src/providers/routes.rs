//! Provider API routes — mirrors server/modules/providers/providers.routes.ts

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde_json::{json, Value};

use crate::db::repos::sessions::SessionsRepo;
use crate::providers::registry::ProviderRegistry;
use crate::shared::types::ProviderAuthStatus;

/// Create the provider API router.
pub fn routes() -> Router<Arc<ProviderRegistry>> {
    Router::new()
        .route("/", get(list_providers))
        .route("/{id}/sessions", get(list_provider_sessions))
        .route("/{id}/skills", get(list_provider_skills))
}

/// GET /api/providers — list all providers with their auth status.
async fn list_providers(
    State(registry): State<Arc<ProviderRegistry>>,
) -> Json<Value> {
    let providers = registry.list_all();
    let mut results = Vec::new();

    for provider in providers {
        let status = provider
            .auth()
            .get_status()
            .await
            .unwrap_or_else(|e| ProviderAuthStatus {
                provider: provider.id(),
                installed: false,
                authenticated: false,
                email: None,
                method: None,
                error: Some(e.to_string()),
            });
        results.push(serde_json::to_value(&status).unwrap_or_default());
    }

    Json(json!({ "success": true, "data": results }))
}

/// GET /api/providers/:id/sessions — list sessions for a provider.
async fn list_provider_sessions(
    State(registry): State<Arc<ProviderRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Validate provider exists
    registry.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": format!("Provider '{}' not found", id) })),
        )
    })?;

    let sessions = SessionsRepo::list_sessions(None);
    let provider_sessions: Vec<Value> = sessions
        .into_iter()
        .filter(|s| s.provider == id)
        .map(|s| {
            json!({
                "sessionId": s.session_id,
                "provider": s.provider,
                "projectPath": s.project_path,
                "customName": s.custom_name,
                "isArchived": s.is_archived == 1,
                "createdAt": s.created_at,
                "updatedAt": s.updated_at,
            })
        })
        .collect();

    Ok(Json(json!({ "success": true, "data": provider_sessions })))
}

/// GET /api/providers/:id/skills — list skills for a provider.
async fn list_provider_skills(
    State(registry): State<Arc<ProviderRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let provider = registry.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": format!("Provider '{}' not found", id) })),
        )
    })?;

    let skills = provider.skills().list_skills(None).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
    })?;

    let skill_values: Vec<Value> = skills
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .collect();

    Ok(Json(json!({ "success": true, "data": skill_values })))
}
