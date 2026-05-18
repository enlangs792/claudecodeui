//! Provider API routes — mirrors server/modules/providers/providers.routes.ts

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    response::Json,
    routing::{delete, get, post},
    Router,
};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::repos::sessions::SessionsRepo;
use crate::providers::registry::ProviderRegistry;
use crate::shared::types::{FetchHistoryOptions, McpScope};

// ── Query / Body types ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct PaginationQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct McpServersQuery {
    #[serde(rename = "workspacePath")]
    workspace_path: Option<String>,
    scope: Option<McpScope>,
}

#[derive(Debug, Deserialize)]
struct McpDeleteQuery {
    scope: Option<McpScope>,
    #[serde(rename = "workspacePath")]
    workspace_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionDeleteQuery {
    force: Option<String>,
    #[serde(rename = "deletedFromDisk")]
    deleted_from_disk: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenameBody {
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionMessagesQuery {
    limit: Option<String>,
    offset: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
    limit: Option<String>,
}

// ── Validators ──────────────────────────────────────────────────────────────────

/// Validate a sessionId parameter (matches TS pattern /^[a-zA-Z0-9._-]{1,120}$/)
fn validate_session_id(session_id: &str) -> bool {
    if session_id.is_empty() || session_id.len() > 120 {
        return false;
    }
    session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// Parse an optional boolean query parameter value.
fn parse_optional_bool(val: Option<&str>) -> Option<bool> {
    match val {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

// ── Route tree ─────────────────────────────────────────────────────────────────

pub fn routes() -> Router<Arc<ProviderRegistry>> {
    Router::new()
        // ── Provider info ──────────────────────────────────────────────────
        .route("/", get(list_providers))
        .route("/{id}/auth/status", get(provider_auth_status))
        .route("/{id}/sessions", get(list_provider_sessions))
        .route("/{id}/skills", get(list_provider_skills))
        // ── MCP routes (per-provider) ──────────────────────────────────────
        .route(
            "/{id}/mcp/servers",
            get(list_mcp_servers).post(upsert_mcp_server),
        )
        .route("/{id}/mcp/servers/{name}", delete(delete_mcp_server))
        // ── MCP routes (global) ────────────────────────────────────────────
        .route("/mcp/servers/global", post(global_add_mcp_server))
        // ── Session routes ─────────────────────────────────────────────────
        .route("/sessions/archived", get(list_archived_sessions))
        .route(
            "/sessions/{sessionId}",
            delete(delete_session).put(rename_session),
        )
        .route("/sessions/{sessionId}/restore", post(restore_session))
        .route(
            "/sessions/{sessionId}/messages",
            get(get_session_messages),
        )
        // ── Search ─────────────────────────────────────────────────────────
        .route("/search/sessions", get(search_sessions))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Provider info endpoints
// ═══════════════════════════════════════════════════════════════════════════════

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
            .unwrap_or_else(|e| crate::shared::types::ProviderAuthStatus {
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

/// GET /api/providers/:id/auth/status — auth status for a specific provider.
async fn provider_auth_status(
    State(registry): State<Arc<ProviderRegistry>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let provider = registry.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(
                json!({ "success": false, "error": format!("Provider '{}' not found", id) }),
            ),
        )
    })?;

    let status = provider.auth().get_status().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({ "success": true, "data": status })))
}

/// GET /api/providers/:id/sessions — list sessions for a provider (with pagination).
async fn list_provider_sessions(
    State(registry): State<Arc<ProviderRegistry>>,
    Path(id): Path<String>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    registry.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(
                json!({ "success": false, "error": format!("Provider '{}' not found", id) }),
            ),
        )
    })?;

    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);

    let all_sessions = SessionsRepo::list_sessions(None);
    let provider_sessions: Vec<Value> = all_sessions
        .into_iter()
        .filter(|s| s.provider == id)
        .skip(offset as usize)
        .take(limit as usize)
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
            Json(
                json!({ "success": false, "error": format!("Provider '{}' not found", id) }),
            ),
        )
    })?;

    let skills = provider
        .skills()
        .list_skills(None)
        .await
        .map_err(|e| {
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

// ═══════════════════════════════════════════════════════════════════════════════
// MCP routes (per-provider)
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /api/providers/:id/mcp/servers — list MCP servers for a provider.
async fn list_mcp_servers(
    State(registry): State<Arc<ProviderRegistry>>,
    Path(id): Path<String>,
    Query(query): Query<McpServersQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let provider = registry.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(
                json!({ "success": false, "error": format!("Provider '{}' not found", id) }),
            ),
        )
    })?;

    if let Some(scope) = query.scope {
        let servers = provider
            .mcp()
            .list_servers_for_scope(scope, query.workspace_path.clone())
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
            })?;

        Ok(Json(json!({
            "success": true,
            "data": { "provider": id, "scope": scope, "servers": servers }
        })))
    } else {
        let grouped = provider
            .mcp()
            .list_servers(query.workspace_path.clone())
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
            })?;

        Ok(Json(json!({
            "success": true,
            "data": { "provider": id, "scopes": grouped }
        })))
    }
}

/// POST /api/providers/:id/mcp/servers — create or update an MCP server.
async fn upsert_mcp_server(
    State(registry): State<Arc<ProviderRegistry>>,
    Path(id): Path<String>,
    Json(payload): Json<crate::shared::types::UpsertProviderMcpServerInput>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let provider = registry.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(
                json!({ "success": false, "error": format!("Provider '{}' not found", id) }),
            ),
        )
    })?;

    let server = provider.mcp().upsert_server(payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "success": true, "data": { "server": server } })),
    ))
}

/// DELETE /api/providers/:id/mcp/servers/:name — delete a named MCP server.
async fn delete_mcp_server(
    State(registry): State<Arc<ProviderRegistry>>,
    Path((id, name)): Path<(String, String)>,
    Query(query): Query<McpDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let provider = registry.get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(
                json!({ "success": false, "error": format!("Provider '{}' not found", id) }),
            ),
        )
    })?;

    let result = provider
        .mcp()
        .remove_server(name, query.scope, query.workspace_path)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": e.to_string() })),
            )
        })?;

    Ok(Json(json!({ "success": true, "data": result })))
}

// ═══════════════════════════════════════════════════════════════════════════════
// MCP routes (global)
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/providers/mcp/servers/global — add an MCP server to all providers.
async fn global_add_mcp_server(
    State(registry): State<Arc<ProviderRegistry>>,
    Json(payload): Json<crate::shared::types::UpsertProviderMcpServerInput>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if payload.scope == Some(McpScope::Local) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "Global MCP add supports only \"user\" or \"project\" scopes.",
                "code": "INVALID_GLOBAL_MCP_SCOPE"
            })),
        ));
    }

    let override_scope = if payload.scope == Some(McpScope::User) {
        McpScope::User
    } else {
        McpScope::Project
    };

    let mut results = Vec::new();
    for p in registry.list_all() {
        let mut input = payload.clone();
        input.scope = Some(override_scope);
        match p.mcp().upsert_server(input).await {
            Ok(server) => results.push(serde_json::to_value(server).unwrap_or_default()),
            Err(e) => {
                results.push(json!({ "error": e.to_string(), "provider": p.id().as_str() }))
            }
        }
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({ "success": true, "data": { "results": results } })),
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Session routes
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /api/providers/sessions/archived — list all archived sessions.
async fn list_archived_sessions() -> Json<Value> {
    let sessions = SessionsRepo::list_archived_sessions();
    let items: Vec<Value> = sessions
        .into_iter()
        .map(|s| {
            json!({
                "sessionId": s.session_id,
                "provider": s.provider,
                "projectPath": s.project_path,
                "customName": s.custom_name,
                "isArchived": true,
                "createdAt": s.created_at,
                "updatedAt": s.updated_at,
            })
        })
        .collect();

    Json(json!({ "success": true, "data": { "sessions": items } }))
}

/// DELETE /api/providers/sessions/:sessionId — delete or archive a session.
async fn delete_session(
    Path(session_id): Path<String>,
    Query(query): Query<SessionDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !validate_session_id(&session_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "success": false, "error": "Invalid sessionId", "code": "INVALID_SESSION_ID" }),
            ),
        ));
    }

    if SessionsRepo::get_by_id(&session_id).is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Session not found" })),
        ));
    }

    let force = parse_optional_bool(query.force.as_deref()).unwrap_or(false);
    let deleted_from_disk =
        parse_optional_bool(query.deleted_from_disk.as_deref()).unwrap_or(force);

    if force || deleted_from_disk {
        if let Some(session) = SessionsRepo::get_by_id(&session_id) {
            if let Some(ref jsonl_path) = session.jsonl_path {
                let path = std::path::Path::new(jsonl_path);
                if path.exists() {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
        }
        SessionsRepo::delete_by_id(&session_id);
    } else {
        SessionsRepo::archive(&session_id);
    }

    Ok(Json(json!({ "success": true, "data": { "sessionId": session_id } })))
}

/// POST /api/providers/sessions/:sessionId/restore — restore an archived session.
async fn restore_session(
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !validate_session_id(&session_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "success": false, "error": "Invalid sessionId", "code": "INVALID_SESSION_ID" }),
            ),
        ));
    }

    if SessionsRepo::get_by_id(&session_id).is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Session not found" })),
        ));
    }

    SessionsRepo::restore(&session_id);

    Ok(Json(json!({ "success": true, "data": { "sessionId": session_id } })))
}

/// PUT /api/providers/sessions/:sessionId — rename a session.
async fn rename_session(
    Path(session_id): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !validate_session_id(&session_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "success": false, "error": "Invalid sessionId", "code": "INVALID_SESSION_ID" }),
            ),
        ));
    }

    let summary = body.summary.as_deref().unwrap_or("").trim().to_string();
    if summary.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "success": false, "error": "Summary is required", "code": "INVALID_SESSION_SUMMARY" }),
            ),
        ));
    }
    if summary.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "success": false, "error": "Summary must not exceed 500 characters", "code": "INVALID_SESSION_SUMMARY" }),
            ),
        ));
    }

    if SessionsRepo::get_by_id(&session_id).is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Session not found" })),
        ));
    }

    SessionsRepo::update_custom_name(&session_id, &summary);

    Ok(Json(json!({
        "success": true,
        "data": { "sessionId": session_id, "customName": summary }
    })))
}

/// GET /api/providers/sessions/:sessionId/messages — get paginated session messages.
async fn get_session_messages(
    State(registry): State<Arc<ProviderRegistry>>,
    Path(session_id): Path<String>,
    Query(query): Query<SessionMessagesQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !validate_session_id(&session_id) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "success": false, "error": "Invalid sessionId", "code": "INVALID_SESSION_ID" }),
            ),
        ));
    }

    let session = SessionsRepo::get_by_id(&session_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Session not found" })),
        )
    })?;

    let provider = registry.get(&session.provider).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(
                json!({ "success": false, "error": format!("Provider '{}' not found", session.provider) }),
            ),
        )
    })?;

    let limit = query.limit.and_then(|s| s.parse::<u32>().ok());
    let offset = query.offset.and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

    let options = FetchHistoryOptions {
        project_path: session.project_path.clone(),
        limit,
        offset: Some(offset),
    };

    let result = provider
        .sessions()
        .fetch_history(session_id, Some(options))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": e.to_string() })),
            )
        })?;

    Ok(Json(serde_json::to_value(&result).unwrap_or_default()))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Search endpoint (SSE)
// ═══════════════════════════════════════════════════════════════════════════════

/// GET /api/providers/search/sessions — search sessions via SSE streaming.
async fn search_sessions(
    Query(query): Query<SearchQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let q = query.q.as_deref().unwrap_or("").trim().to_lowercase();
    let limit = query
        .limit
        .and_then(|s| s.parse::<usize>().ok())
        .map(|n| n.clamp(1, 100))
        .unwrap_or(50);

    let mut events: Vec<Result<Event, Infallible>> = Vec::new();

    if q.len() < 2 {
        events.push(Ok(
            Event::default()
                .event("error")
                .data(r#"{"error":"Query must be at least 2 characters"}"#),
        ));
        events.push(Ok(Event::default().event("done").data("{}")));
        return Sse::new(stream::iter(events)).keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text(r#"{"type":"keepalive"}"#),
        );
    }

    // Collect both active and archived sessions
    let mut all_sessions = SessionsRepo::list_sessions(None);
    all_sessions.extend(SessionsRepo::list_archived_sessions());

    let mut total_matches = 0usize;
    let mut results_sent = 0usize;

    for session in &all_sessions {
        let session_id_lower = session.session_id.to_lowercase();
        let custom_name_lower = session
            .custom_name
            .as_deref()
            .unwrap_or("")
            .to_lowercase();

        if session_id_lower.contains(&q) || custom_name_lower.contains(&q) {
            total_matches += 1;
            if results_sent < limit {
                let project_result = json!({
                    "sessionId": session.session_id,
                    "provider": session.provider,
                    "customName": session.custom_name,
                    "projectPath": session.project_path,
                    "isArchived": session.is_archived == 1,
                    "createdAt": session.created_at,
                    "updatedAt": session.updated_at,
                });

                events.push(Ok(
                    Event::default()
                        .event("result")
                        .data(
                            serde_json::to_string(&json!({
                                "projectResult": project_result,
                                "totalMatches": total_matches,
                                "scannedProjects": 0,
                                "totalProjects": 0
                            }))
                            .unwrap_or_default(),
                        ),
                ));
                results_sent += 1;
            }
        }
    }

    events.push(Ok(Event::default().event("done").data("{}")));

    Sse::new(stream::iter(events)).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text(r#"{"type":"keepalive"}"#),
    )
}
