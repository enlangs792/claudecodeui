//! Settings routes — mirrors server/routes/settings.js
//!
//! Endpoints mounted at /api/settings:
//!
//! GET/PUT /                               — app config key-value pairs
//! GET     /api-keys                       — list API keys
//! POST    /api-keys                       — create an API key
//! DELETE  /api-keys/:key_id               — delete an API key
//! PATCH   /api-keys/:key_id/toggle        — toggle API key active status
//! GET     /credentials                    — list credentials (?type= filter)
//! POST    /credentials                    — create a credential
//! DELETE  /credentials/:credential_id     — delete a credential
//! PATCH   /credentials/:credential_id/toggle — toggle credential active
//! GET/PUT /notification-preferences       — user notification preferences
//! GET     /push/vapid-public-key          — VAPID public key
//! POST    /push/subscribe                 — register a push subscription
//! POST    /push/unsubscribe               — remove a push subscription
//! GET     /server-env                     — server platform info

use axum::{
    Extension,
    extract::{Path, Query},
    http::StatusCode,
    response::Json,
    routing::{get, patch, post, delete},
    Router,
};
use base64::Engine;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::auth::middleware::AuthUser;
use crate::db::connection;

pub fn routes() -> Router {
    Router::new()
        // ── Basic app config ──────────────────────────────────────────────
        .route("/", get(get_settings).put(update_settings))
        // ── API Keys ──────────────────────────────────────────────────────
        .route("/api-keys", get(list_api_keys).post(create_api_key))
        .route("/api-keys/:key_id", delete(delete_api_key))
        .route("/api-keys/:key_id/toggle", patch(toggle_api_key))
        // ── Credentials ───────────────────────────────────────────────────
        .route("/credentials", get(list_credentials).post(create_credential))
        .route("/credentials/:credential_id", delete(delete_credential))
        .route("/credentials/:credential_id/toggle", patch(toggle_credential))
        // ── Notification preferences ──────────────────────────────────────
        .route(
            "/notification-preferences",
            get(get_notification_prefs).put(update_notification_prefs),
        )
        // ── Push subscriptions ────────────────────────────────────────────
        .route("/push/vapid-public-key", get(get_vapid_public_key))
        .route("/push/subscribe", post(subscribe_push))
        .route("/push/unsubscribe", post(unsubscribe_push))
        // ── Server info ───────────────────────────────────────────────────
        .route("/server-env", get(get_server_env))
}

// ═════════════════════════════════════════════════════════════════════════════
// Basic App Config
// ═════════════════════════════════════════════════════════════════════════════

/// GET /api/settings — return all stored app config key-value pairs
async fn get_settings(
    Extension(_user): Extension<AuthUser>,
) -> Json<Value> {
    // AppConfigRepo only provides get(key) — for a full listing we return
    // a curated set of known config keys.
    let known_keys = [
        "theme",
        "fontSize",
        "language",
        "autoSave",
        "tabSize",
    ];

    let mut settings = HashMap::new();
    for key in &known_keys {
        if let Some(value) = crate::db::repos::app_config::AppConfigRepo::get(key) {
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
        crate::db::repos::app_config::AppConfigRepo::set(key, value);
    }

    Json(json!({
        "success": true,
        "message": "Settings updated successfully"
    }))
}

// ═════════════════════════════════════════════════════════════════════════════
// API Keys Management
// ═════════════════════════════════════════════════════════════════════════════

/// Truncate an API key for safe listing (show first 10 chars + "...")
fn sanitize_api_key(api_key: &str) -> String {
    if api_key.len() > 10 {
        format!("{}...", &api_key[..10])
    } else {
        api_key.to_string()
    }
}

/// Generate a cryptographically random API key with `ck_` prefix
fn generate_api_key() -> String {
    let part1 = uuid::Uuid::new_v4().to_string().replace('-', "");
    let part2 = uuid::Uuid::new_v4().to_string().replace('-', "");
    format!("ck_{}{}", part1, part2)
}

#[derive(Debug, Serialize)]
struct ApiKeySanitized {
    id: i64,
    key_name: String,
    api_key: String,
    created_at: String,
    last_used: Option<String>,
    is_active: i32,
}

/// GET /api/settings/api-keys — list all API keys for the authenticated user
async fn list_api_keys(
    Extension(user): Extension<AuthUser>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let keys = connection::with_connection(|db| {
        let mut stmt = db
            .prepare(
                "SELECT id, user_id, key_name, api_key, created_at, last_used, is_active
                 FROM api_keys WHERE user_id = ?1 ORDER BY created_at DESC",
            )
            .map_err(|e| format!("Failed to prepare query: {e}"))?;

        let rows = stmt
            .query_map(params![user.id], |row| {
                Ok(ApiKeySanitized {
                    id: row.get(0)?,
                    key_name: row.get(2)?,
                    api_key: sanitize_api_key(&row.get::<_, String>(3)?),
                    created_at: row.get(4)?,
                    last_used: row.get(5)?,
                    is_active: row.get(6)?,
                })
            })
            .map_err(|e| format!("Failed to query API keys: {e}"))?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        Ok::<_, String>(rows)
    })
    .map_err(|e: String| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to fetch API keys: {e}")})),
        )
    })?;

    Ok(Json(json!({ "apiKeys": keys })))
}

#[derive(Debug, Deserialize)]
struct CreateApiKeyBody {
    #[serde(rename = "keyName")]
    key_name: Option<String>,
}

/// POST /api/settings/api-keys — create a new API key
async fn create_api_key(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<CreateApiKeyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key_name = body.key_name.unwrap_or_default();
    if key_name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Key name is required"})),
        ));
    }

    let api_key = generate_api_key();

    let id = crate::db::repos::api_keys::ApiKeysRepo::create(user.id, key_name.trim(), &api_key);

    Ok(Json(json!({
        "success": true,
        "apiKey": {
            "id": id,
            "keyName": key_name.trim(),
            "apiKey": api_key
        }
    })))
}

/// DELETE /api/settings/api-keys/:key_id — hard-delete an API key
async fn delete_api_key(
    Extension(user): Extension<AuthUser>,
    Path(key_id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = connection::with_connection(|db| {
        let affected = db
            .execute(
                "DELETE FROM api_keys WHERE id = ?1 AND user_id = ?2",
                params![key_id, user.id],
            )
            .unwrap_or(0);
        affected > 0
    });

    if deleted {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "API key not found"})),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct ToggleActiveBody {
    #[serde(rename = "isActive")]
    is_active: bool,
}

/// PATCH /api/settings/api-keys/:key_id/toggle — enable or disable an API key
async fn toggle_api_key(
    Extension(user): Extension<AuthUser>,
    Path(key_id): Path<i64>,
    Json(body): Json<ToggleActiveBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let updated = connection::with_connection(|db| {
        let affected = db
            .execute(
                "UPDATE api_keys SET is_active = ?1 WHERE id = ?2 AND user_id = ?3",
                params![body.is_active as i32, key_id, user.id],
            )
            .unwrap_or(0);
        affected > 0
    });

    if updated {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "API key not found"})),
        ))
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Generic Credentials Management
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct CredentialsQuery {
    #[serde(rename = "type")]
    cred_type: Option<String>,
}

/// GET /api/settings/credentials — list credentials (optionally filtered by type)
async fn list_credentials(
    Extension(user): Extension<AuthUser>,
    Query(params): Query<CredentialsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let credentials = if let Some(ref cred_type) = params.cred_type {
        connection::with_connection(|db| {
            let mut stmt = db
                .prepare(
                    "SELECT id, credential_name, credential_type, description, created_at, is_active
                     FROM user_credentials WHERE user_id = ?1 AND credential_type = ?2
                     ORDER BY created_at DESC",
                )
                .map_err(|e| format!("Failed to prepare query: {e}"))?;

            let rows = stmt
                .query_map(params![user.id, cred_type], |row| {
                    Ok(crate::shared::types::CredentialPublicRow {
                        id: row.get(0)?,
                        credential_name: row.get(1)?,
                        credential_type: row.get(2)?,
                        description: row.get(3)?,
                        created_at: row.get(4)?,
                        is_active: row.get(5)?,
                    })
                })
                .map_err(|e| format!("Failed to query credentials: {e}"))?
                .filter_map(|r| r.ok())
                .collect::<Vec<_>>();

            Ok::<_, String>(rows)
        })
    } else {
        connection::with_connection(|db| {
            let mut stmt = db
                .prepare(
                    "SELECT id, credential_name, credential_type, description, created_at, is_active
                     FROM user_credentials WHERE user_id = ?1
                     ORDER BY created_at DESC",
                )
                .map_err(|e| format!("Failed to prepare query: {e}"))?;

            let rows = stmt
                .query_map(params![user.id], |row| {
                    Ok(crate::shared::types::CredentialPublicRow {
                        id: row.get(0)?,
                        credential_name: row.get(1)?,
                        credential_type: row.get(2)?,
                        description: row.get(3)?,
                        created_at: row.get(4)?,
                        is_active: row.get(5)?,
                    })
                })
                .map_err(|e| format!("Failed to query credentials: {e}"))?
                .filter_map(|r| r.ok())
                .collect::<Vec<_>>();

            Ok::<_, String>(rows)
        })
    }
    .map_err(|e: String| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to fetch credentials: {e}")})),
        )
    })?;

    Ok(Json(json!({ "credentials": credentials })))
}

#[derive(Debug, Deserialize)]
struct CreateCredentialBody {
    #[serde(rename = "credentialName")]
    credential_name: Option<String>,
    #[serde(rename = "credentialType")]
    credential_type: Option<String>,
    #[serde(rename = "credentialValue")]
    credential_value: Option<String>,
    description: Option<String>,
}

/// POST /api/settings/credentials — create a new credential
async fn create_credential(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<CreateCredentialBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let name = match body.credential_name {
        Some(ref n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Credential name is required"})),
            ))
        }
    };

    let cred_type = match body.credential_type {
        Some(ref t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Credential type is required"})),
            ))
        }
    };

    let cred_value = match body.credential_value {
        Some(ref v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Credential value is required"})),
            ))
        }
    };

    let description = body
        .description
        .as_ref()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());

    let result = crate::db::repos::credentials::CredentialsRepo::create(
        user.id,
        &name,
        &cred_type,
        &cred_value,
        description.as_deref(),
    );

    Ok(Json(json!({
        "success": true,
        "credential": {
            "id": result.id,
            "credentialName": result.credential_name,
            "credentialType": result.credential_type
        }
    })))
}

/// DELETE /api/settings/credentials/:credential_id — hard-delete a credential
async fn delete_credential(
    Extension(user): Extension<AuthUser>,
    Path(credential_id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = connection::with_connection(|db| {
        let affected = db
            .execute(
                "DELETE FROM user_credentials WHERE id = ?1 AND user_id = ?2",
                params![credential_id, user.id],
            )
            .unwrap_or(0);
        affected > 0
    });

    if deleted {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Credential not found"})),
        ))
    }
}

/// PATCH /api/settings/credentials/:credential_id/toggle — enable or disable a credential
async fn toggle_credential(
    Extension(user): Extension<AuthUser>,
    Path(credential_id): Path<i64>,
    Json(body): Json<ToggleActiveBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let updated = connection::with_connection(|db| {
        let affected = db
            .execute(
                "UPDATE user_credentials SET is_active = ?1 WHERE id = ?2 AND user_id = ?3",
                params![body.is_active as i32, credential_id, user.id],
            )
            .unwrap_or(0);
        affected > 0
    });

    if updated {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Credential not found"})),
        ))
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Notification Preferences
// ═════════════════════════════════════════════════════════════════════════════

/// Normalize a raw JSON value into the canonical notification prefs shape
fn normalize_notification_prefs(value: &Value) -> Value {
    let default_channels = json!({"inApp": false, "webPush": false});
    let default_events = json!({"actionRequired": true, "stop": true, "error": true});

    let channels = value
        .get("channels")
        .and_then(|c| c.as_object())
        .map(|_| {
            json!({
                "inApp": value["channels"]["inApp"].as_bool().unwrap_or(false),
                "webPush": value["channels"]["webPush"].as_bool().unwrap_or(false)
            })
        })
        .unwrap_or(default_channels);

    let events = value.get("events").and_then(|e| e.as_object()).map(|_| {
        json!({
            "actionRequired": value["events"]["actionRequired"].as_bool().unwrap_or(true),
            "stop": value["events"]["stop"].as_bool().unwrap_or(true),
            "error": value["events"]["error"].as_bool().unwrap_or(true)
        })
    }).unwrap_or(default_events);

    json!({"channels": channels, "events": events})
}

/// Get or create default notification preferences for a user
fn get_or_create_prefs(user_id: i64) -> Value {
    connection::with_connection(|db| {
        let row: Option<String> = db
            .query_row(
                "SELECT preferences_json FROM user_notification_preferences WHERE user_id = ?1",
                params![user_id],
                |row| row.get(0),
            )
            .ok();

        match row {
            Some(json_str) => {
                serde_json::from_str(&json_str)
                    .map(|v: Value| normalize_notification_prefs(&v))
                    .unwrap_or_else(|_| {
                        let defaults = normalize_notification_prefs(&Value::Null);
                        let json_str = serde_json::to_string(&defaults).unwrap_or_default();
                        db.execute(
                            "INSERT OR REPLACE INTO user_notification_preferences (user_id, preferences_json, updated_at)
                             VALUES (?1, ?2, CURRENT_TIMESTAMP)",
                            params![user_id, json_str],
                        )
                        .ok();
                        defaults
                    })
            }
            None => {
                let defaults = normalize_notification_prefs(&Value::Null);
                let json_str = serde_json::to_string(&defaults).unwrap_or_default();
                db.execute(
                    "INSERT INTO user_notification_preferences (user_id, preferences_json, updated_at)
                     VALUES (?1, ?2, CURRENT_TIMESTAMP)",
                    params![user_id, json_str],
                )
                .ok();
                defaults
            }
        }
    })
}

/// GET /api/settings/notification-preferences — get notification preferences
async fn get_notification_prefs(
    Extension(user): Extension<AuthUser>,
) -> Json<Value> {
    let preferences = get_or_create_prefs(user.id);
    Json(json!({ "success": true, "preferences": preferences }))
}

/// PUT /api/settings/notification-preferences — update notification preferences
async fn update_notification_prefs(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let normalized = normalize_notification_prefs(&body);
    let json_str = serde_json::to_string(&normalized).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to serialize preferences: {e}")})),
        )
    })?;

    connection::with_connection(|db| {
        db.execute(
            "INSERT INTO user_notification_preferences (user_id, preferences_json, updated_at)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(user_id) DO UPDATE SET
               preferences_json = excluded.preferences_json,
               updated_at = CURRENT_TIMESTAMP",
            params![user.id, json_str],
        )
        .ok();
    });

    Ok(Json(json!({ "success": true, "preferences": normalized })))
}

// ═════════════════════════════════════════════════════════════════════════════
// Push Subscription Management
// ═════════════════════════════════════════════════════════════════════════════

/// Ensure VAPID keys exist in the database, returning the public key.
/// Checks env vars first, then stored keys, then generates a fresh pair.
fn ensure_and_get_vapid_public_key() -> String {
    // 1. Env var takes precedence
    if let Ok(key) = std::env::var("VAPID_PUBLIC_KEY") {
        if !key.is_empty() {
            return key;
        }
    }

    // 2. Read from database
    let cached = connection::with_connection(|db| {
        db.query_row(
            "SELECT public_key FROM vapid_keys ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
    });

    if let Some(key) = cached {
        return key;
    }

    // 3. Generate new keys and store them
    let (public_key, private_key) = generate_vapid_key_pair();
    connection::with_connection(|db| {
        db.execute(
            "INSERT INTO vapid_keys (public_key, private_key) VALUES (?1, ?2)",
            params![public_key, private_key],
        )
        .ok();
    });

    public_key
}

/// Generate a VAPID-like key pair using available crypto.
/// Uses base64-encoded random UUIDs as placeholder VAPID keys.
/// For production, set VAPID_PUBLIC_KEY / VAPID_PRIVATE_KEY env vars.
fn generate_vapid_key_pair() -> (String, String) {
    use base64::engine::general_purpose::STANDARD as BASE64;

    let pub_input = uuid::Uuid::new_v4().to_string().repeat(3);
    let priv_input = uuid::Uuid::new_v4().to_string().repeat(3);

    (BASE64.encode(pub_input), BASE64.encode(priv_input))
}

/// GET /api/settings/push/vapid-public-key — return the VAPID public key
async fn get_vapid_public_key() -> Json<Value> {
    let public_key = ensure_and_get_vapid_public_key();
    Json(json!({ "publicKey": public_key }))
}

#[derive(Debug, Deserialize)]
struct SubscribeBody {
    endpoint: Option<String>,
    keys: Option<SubscribeKeys>,
}

#[derive(Debug, Deserialize)]
struct SubscribeKeys {
    p256dh: Option<String>,
    auth: Option<String>,
}

/// POST /api/settings/push/subscribe — register a push subscription
async fn subscribe_push(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<SubscribeBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let endpoint = match body.endpoint {
        Some(ref e) if !e.is_empty() => e.clone(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing subscription fields"})),
            ))
        }
    };

    let keys = body.keys.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing subscription fields"})),
        )
    })?;

    let p256dh = keys.p256dh.as_deref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing subscription fields"})),
        )
    })?;

    let auth = keys.auth.as_deref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing subscription fields"})),
        )
    })?;

    // Upsert push subscription
    connection::with_connection(|db| {
        db.execute(
            "INSERT INTO push_subscriptions (user_id, endpoint, keys_p256dh, keys_auth)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(endpoint) DO UPDATE SET
               user_id = excluded.user_id,
               keys_p256dh = excluded.keys_p256dh,
               keys_auth = excluded.keys_auth",
            params![user.id, endpoint, p256dh, auth],
        )
        .ok();
    });

    // Enable webPush in notification preferences (matching TS behavior)
    let current_prefs = get_or_create_prefs(user.id);
    if current_prefs["channels"]["webPush"].as_bool() != Some(true) {
        let mut updated = current_prefs.clone();
        if let Some(obj) = updated.as_object_mut() {
            if let Some(channels) = obj.get_mut("channels").and_then(|c| c.as_object_mut()) {
                channels.insert("webPush".into(), json!(true));
            }
        }
        let json_str = serde_json::to_string(&updated).unwrap_or_default();
        connection::with_connection(|db| {
            db.execute(
                "INSERT INTO user_notification_preferences (user_id, preferences_json, updated_at)
                 VALUES (?1, ?2, CURRENT_TIMESTAMP)
                 ON CONFLICT(user_id) DO UPDATE SET
                   preferences_json = excluded.preferences_json,
                   updated_at = CURRENT_TIMESTAMP",
                params![user.id, json_str],
            )
            .ok();
        });
    }

    Ok(Json(json!({ "success": true })))
}

#[derive(Debug, Deserialize)]
struct UnsubscribeBody {
    endpoint: Option<String>,
}

/// POST /api/settings/push/unsubscribe — remove a push subscription
async fn unsubscribe_push(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<UnsubscribeBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let endpoint = match body.endpoint {
        Some(ref e) if !e.is_empty() => e.clone(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing endpoint"})),
            ))
        }
    };

    // Remove subscription
    connection::with_connection(|db| {
        db.execute(
            "DELETE FROM push_subscriptions WHERE endpoint = ?1",
            params![endpoint],
        )
        .ok();
    });

    // Disable webPush in preferences (matching TS behavior)
    let current_prefs = get_or_create_prefs(user.id);
    if current_prefs["channels"]["webPush"].as_bool() == Some(true) {
        let mut updated = current_prefs;
        if let Some(obj) = updated.as_object_mut() {
            if let Some(channels) = obj.get_mut("channels").and_then(|c| c.as_object_mut()) {
                channels.insert("webPush".into(), json!(false));
            }
        }
        let json_str = serde_json::to_string(&updated).unwrap_or_default();
        connection::with_connection(|db| {
            db.execute(
                "INSERT INTO user_notification_preferences (user_id, preferences_json, updated_at)
                 VALUES (?1, ?2, CURRENT_TIMESTAMP)
                 ON CONFLICT(user_id) DO UPDATE SET
                   preferences_json = excluded.preferences_json,
                   updated_at = CURRENT_TIMESTAMP",
                params![user.id, json_str],
            )
            .ok();
        });
    }

    Ok(Json(json!({ "success": true })))
}

// ═════════════════════════════════════════════════════════════════════════════
// Server Environment
// ═════════════════════════════════════════════════════════════════════════════

/// GET /api/settings/server-env — return server platform information
async fn get_server_env() -> Json<Value> {
    Json(json!({ "platform": std::env::consts::OS }))
}
