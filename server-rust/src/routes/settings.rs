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
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::auth::middleware::AuthUser;
use crate::db::{connection, models, schema};

pub fn routes() -> Router {
    Router::new()
        .route("/", get(get_settings).put(update_settings))
        .route("/api-keys", get(list_api_keys).post(create_api_key))
        .route("/api-keys/:key_id", delete(delete_api_key))
        .route("/api-keys/:key_id/toggle", patch(toggle_api_key))
        .route("/credentials", get(list_credentials).post(create_credential))
        .route("/credentials/:credential_id", delete(delete_credential))
        .route("/credentials/:credential_id/toggle", patch(toggle_credential))
        .route(
            "/notification-preferences",
            get(get_notification_prefs).put(update_notification_prefs),
        )
        .route("/push/vapid-public-key", get(get_vapid_public_key))
        .route("/push/subscribe", post(subscribe_push))
        .route("/push/unsubscribe", post(unsubscribe_push))
        .route("/server-env", get(get_server_env))
}

// ═════════════════════════════════════════════════════════════════════════════
// Basic App Config
// ═════════════════════════════════════════════════════════════════════════════

async fn get_settings(
    Extension(_user): Extension<AuthUser>,
) -> Json<Value> {
    let known_keys = ["theme", "fontSize", "language", "autoSave", "tabSize"];
    let mut settings = HashMap::new();
    for key in &known_keys {
        if let Some(value) = crate::db::repos::app_config::AppConfigRepo::get(key) {
            settings.insert(key.to_string(), value);
        }
    }
    Json(json!({ "settings": settings }))
}

#[derive(Debug, Deserialize)]
struct UpdateSettingsBody {
    #[serde(flatten)]
    settings: HashMap<String, String>,
}

async fn update_settings(
    Extension(_user): Extension<AuthUser>,
    Json(body): Json<UpdateSettingsBody>,
) -> Json<Value> {
    for (key, value) in &body.settings {
        crate::db::repos::app_config::AppConfigRepo::set(key, value);
    }
    Json(json!({ "success": true, "message": "Settings updated successfully" }))
}

// ═════════════════════════════════════════════════════════════════════════════
// API Keys Management
// ═════════════════════════════════════════════════════════════════════════════

fn sanitize_api_key(api_key: &str) -> String {
    if api_key.len() > 10 {
        format!("{}...", &api_key[..10])
    } else {
        api_key.to_string()
    }
}

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
    created_at: Option<chrono::NaiveDateTime>,
    last_used: Option<chrono::NaiveDateTime>,
    is_active: bool,
}

async fn list_api_keys(
    Extension(user): Extension<AuthUser>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let keys: Vec<ApiKeySanitized> = connection::with_db(|conn| {
        use schema::api_keys;
        api_keys::table
            .filter(api_keys::user_id.eq(user.id))
            .order(api_keys::created_at.desc())
            .select((
                api_keys::id,
                api_keys::key_name,
                api_keys::api_key,
                api_keys::created_at,
                api_keys::last_used,
                api_keys::is_active,
            ))
            .load::<(i64, String, String, Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>, bool)>(conn)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, key_name, api_key, created_at, last_used, is_active)| {
                ApiKeySanitized {
                    id,
                    key_name,
                    api_key: sanitize_api_key(&api_key),
                    created_at,
                    last_used,
                    is_active,
                }
            })
            .collect()
    });

    Ok(Json(json!({ "apiKeys": keys })))
}

#[derive(Debug, Deserialize)]
struct CreateApiKeyBody {
    #[serde(rename = "keyName")]
    key_name: Option<String>,
}

async fn create_api_key(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<CreateApiKeyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key_name = body.key_name.unwrap_or_default();
    if key_name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Key name is required"}))));
    }
    let api_key = generate_api_key();
    let id = crate::db::repos::api_keys::ApiKeysRepo::create(user.id, key_name.trim(), &api_key);
    Ok(Json(json!({
        "success": true,
        "apiKey": { "id": id, "keyName": key_name.trim(), "apiKey": api_key }
    })))
}

async fn delete_api_key(
    Extension(user): Extension<AuthUser>,
    Path(key_id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = connection::with_db(|conn| {
        use schema::api_keys;
        diesel::delete(
            schema::api_keys::table
                .filter(schema::api_keys::id.eq(key_id))
                .filter(schema::api_keys::user_id.eq(user.id)),
        )
        .execute(conn)
        .unwrap_or(0) > 0
    });

    if deleted {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((StatusCode::NOT_FOUND, Json(json!({"error": "API key not found"}))))
    }
}

#[derive(Debug, Deserialize)]
struct ToggleActiveBody {
    #[serde(rename = "isActive")]
    is_active: bool,
}

async fn toggle_api_key(
    Extension(user): Extension<AuthUser>,
    Path(key_id): Path<i64>,
    Json(body): Json<ToggleActiveBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let updated = connection::with_db(|conn| {
        use schema::api_keys;
        diesel::update(
            schema::api_keys::table
                .filter(schema::api_keys::id.eq(key_id))
                .filter(schema::api_keys::user_id.eq(user.id)),
        )
        .set(schema::api_keys::is_active.eq(body.is_active))
        .execute(conn)
        .unwrap_or(0) > 0
    });

    if updated {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((StatusCode::NOT_FOUND, Json(json!({"error": "API key not found"}))))
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

async fn list_credentials(
    Extension(user): Extension<AuthUser>,
    Query(params): Query<CredentialsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let credentials = connection::with_db(|conn| {
        use schema::user_credentials;
        let mut query = user_credentials::table
            .filter(user_credentials::user_id.eq(user.id))
            .order(user_credentials::created_at.desc())
            .select((
                user_credentials::id,
                user_credentials::credential_name,
                user_credentials::credential_type,
                user_credentials::description,
                user_credentials::created_at,
                user_credentials::is_active,
            ))
            .into_boxed();

        if let Some(ref cred_type) = params.cred_type {
            query = query.filter(user_credentials::credential_type.eq(cred_type));
        }

        query
            .load::<(i64, String, String, Option<String>, Option<chrono::NaiveDateTime>, bool)>(conn)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, credential_name, credential_type, description, created_at, is_active)| {
                crate::shared::types::CredentialPublicRow {
                    id,
                    credential_name,
                    credential_type,
                    description,
                    created_at: created_at
                        .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string())
                        .unwrap_or_default(),
                    is_active: is_active as i32,
                }
            })
            .collect::<Vec<_>>()
    });

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

async fn create_credential(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<CreateCredentialBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let name = match body.credential_name {
        Some(ref n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Credential name is required"})))),
    };
    let cred_type = match body.credential_type {
        Some(ref t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Credential type is required"})))),
    };
    let cred_value = match body.credential_value {
        Some(ref v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Credential value is required"})))),
    };
    let description = body.description.as_ref().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());

    let result = crate::db::repos::credentials::CredentialsRepo::create(
        user.id, &name, &cred_type, &cred_value, description.as_deref(),
    );
    Ok(Json(json!({ "success": true, "credential": { "id": result.id, "credentialName": result.credential_name, "credentialType": result.credential_type } })))
}

async fn delete_credential(
    Extension(user): Extension<AuthUser>,
    Path(credential_id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = connection::with_db(|conn| {
        use schema::user_credentials;
        diesel::delete(
            schema::user_credentials::table
                .filter(schema::user_credentials::id.eq(credential_id))
                .filter(schema::user_credentials::user_id.eq(user.id)),
        )
        .execute(conn)
        .unwrap_or(0) > 0
    });

    if deleted {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((StatusCode::NOT_FOUND, Json(json!({"error": "Credential not found"}))))
    }
}

async fn toggle_credential(
    Extension(user): Extension<AuthUser>,
    Path(credential_id): Path<i64>,
    Json(body): Json<ToggleActiveBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let updated = connection::with_db(|conn| {
        use schema::user_credentials;
        diesel::update(
            schema::user_credentials::table
                .filter(schema::user_credentials::id.eq(credential_id))
                .filter(schema::user_credentials::user_id.eq(user.id)),
        )
        .set(schema::user_credentials::is_active.eq(body.is_active))
        .execute(conn)
        .unwrap_or(0) > 0
    });

    if updated {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((StatusCode::NOT_FOUND, Json(json!({"error": "Credential not found"}))))
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Notification Preferences
// ═════════════════════════════════════════════════════════════════════════════

fn normalize_notification_prefs(value: &Value) -> Value {
    let default_channels = json!({"inApp": false, "webPush": false});
    let default_events = json!({"actionRequired": true, "stop": true, "error": true});
    let channels = value.get("channels").and_then(|c| c.as_object()).map(|_| {
        json!({"inApp": value["channels"]["inApp"].as_bool().unwrap_or(false), "webPush": value["channels"]["webPush"].as_bool().unwrap_or(false)})
    }).unwrap_or(default_channels);
    let events = value.get("events").and_then(|e| e.as_object()).map(|_| {
        json!({"actionRequired": value["events"]["actionRequired"].as_bool().unwrap_or(true), "stop": value["events"]["stop"].as_bool().unwrap_or(true), "error": value["events"]["error"].as_bool().unwrap_or(true)})
    }).unwrap_or(default_events);
    json!({"channels": channels, "events": events})
}

fn get_or_create_prefs(user_id: i64) -> Value {
    connection::with_db(|conn| {
        use schema::user_notification_preferences;

        let row: Option<String> = user_notification_preferences::table
            .filter(user_notification_preferences::user_id.eq(user_id))
            .select(user_notification_preferences::preferences_json)
            .first::<String>(conn)
            .ok();

        match row {
            Some(json_str) => {
                serde_json::from_str(&json_str)
                    .map(|v: Value| normalize_notification_prefs(&v))
                    .unwrap_or_else(|_| {
                        let defaults = normalize_notification_prefs(&Value::Null);
                        let json_str = serde_json::to_string(&defaults).unwrap_or_default();
                        let new_pref = models::NewNotificationPreference {
                            user_id,
                            preferences_json: json_str,
                        };
                        diesel::insert_into(user_notification_preferences::table)
                            .values(&new_pref)
                            .on_conflict(user_notification_preferences::user_id)
                            .do_update()
                            .set((
                                user_notification_preferences::preferences_json.eq(&new_pref.preferences_json),
                                user_notification_preferences::updated_at.eq(chrono::Utc::now().naive_utc()),
                            ))
                            .execute(conn)
                            .ok();
                        defaults
                    })
            }
            None => {
                let defaults = normalize_notification_prefs(&Value::Null);
                let json_str = serde_json::to_string(&defaults).unwrap_or_default();
                let new_pref = models::NewNotificationPreference {
                    user_id,
                    preferences_json: json_str,
                };
                diesel::insert_into(user_notification_preferences::table)
                    .values(&new_pref)
                    .execute(conn)
                    .ok();
                defaults
            }
        }
    })
}

async fn get_notification_prefs(
    Extension(user): Extension<AuthUser>,
) -> Json<Value> {
    let preferences = get_or_create_prefs(user.id);
    Json(json!({ "success": true, "preferences": preferences }))
}

async fn update_notification_prefs(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let normalized = normalize_notification_prefs(&body);
    let json_str = serde_json::to_string(&normalized).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to serialize preferences: {e}")})))
    })?;

    connection::with_db(|conn| {
        use schema::user_notification_preferences;
        let new_pref = models::NewNotificationPreference {
            user_id: user.id,
            preferences_json: json_str,
        };
        diesel::insert_into(user_notification_preferences::table)
            .values(&new_pref)
            .on_conflict(user_notification_preferences::user_id)
            .do_update()
            .set((
                user_notification_preferences::preferences_json.eq(&new_pref.preferences_json),
                user_notification_preferences::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .execute(conn)
            .ok();
    });

    Ok(Json(json!({ "success": true, "preferences": normalized })))
}

// ═════════════════════════════════════════════════════════════════════════════
// Push Subscription Management
// ═════════════════════════════════════════════════════════════════════════════

fn ensure_and_get_vapid_public_key() -> String {
    if let Ok(key) = std::env::var("VAPID_PUBLIC_KEY") {
        if !key.is_empty() { return key; }
    }

    let cached = connection::with_db(|conn| {
        use schema::vapid_keys;
        vapid_keys::table
            .order(vapid_keys::id.desc())
            .select(vapid_keys::public_key)
            .first::<String>(conn)
            .ok()
    });

    if let Some(key) = cached { return key; }

    let (public_key, private_key) = generate_vapid_key_pair();
    connection::with_db(|conn| {
        use schema::vapid_keys;
        diesel::insert_into(vapid_keys::table)
            .values(&models::NewVapidKey {
                public_key: public_key.clone(),
                private_key,
            })
            .execute(conn)
            .ok();
    });

    public_key
}

fn generate_vapid_key_pair() -> (String, String) {
    use base64::engine::general_purpose::STANDARD as BASE64;
    let pub_input = uuid::Uuid::new_v4().to_string().repeat(3);
    let priv_input = uuid::Uuid::new_v4().to_string().repeat(3);
    (BASE64.encode(pub_input), BASE64.encode(priv_input))
}

async fn get_vapid_public_key() -> Json<Value> {
    Json(json!({ "publicKey": ensure_and_get_vapid_public_key() }))
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

async fn subscribe_push(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<SubscribeBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let endpoint = match body.endpoint {
        Some(ref e) if !e.is_empty() => e.clone(),
        _ => return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Missing subscription fields"})))),
    };
    let keys = body.keys.as_ref().ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing subscription fields"})))
    })?;
    let p256dh = keys.p256dh.as_deref().ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing subscription fields"})))
    })?;
    let auth = keys.auth.as_deref().ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing subscription fields"})))
    })?;

    connection::with_db(|conn| {
        use schema::push_subscriptions;
        let new_sub = models::NewPushSubscription {
            user_id: user.id,
            endpoint: endpoint.clone(),
            keys_p256dh: p256dh.to_string(),
            keys_auth: auth.to_string(),
        };
        diesel::insert_into(push_subscriptions::table)
            .values(&new_sub)
            .on_conflict(push_subscriptions::endpoint)
            .do_update()
            .set((
                push_subscriptions::user_id.eq(user.id),
                push_subscriptions::keys_p256dh.eq(p256dh),
                push_subscriptions::keys_auth.eq(auth),
            ))
            .execute(conn)
            .ok();
    });

    let current_prefs = get_or_create_prefs(user.id);
    if current_prefs["channels"]["webPush"].as_bool() != Some(true) {
        let mut updated = current_prefs.clone();
        if let Some(obj) = updated.as_object_mut() {
            if let Some(channels) = obj.get_mut("channels").and_then(|c| c.as_object_mut()) {
                channels.insert("webPush".into(), json!(true));
            }
        }
        let json_str = serde_json::to_string(&updated).unwrap_or_default();
        connection::with_db(|conn| {
            use schema::user_notification_preferences;
            let new_pref = models::NewNotificationPreference {
                user_id: user.id,
                preferences_json: json_str,
            };
            diesel::insert_into(user_notification_preferences::table)
                .values(&new_pref)
                .on_conflict(user_notification_preferences::user_id)
                .do_update()
                .set((
                    user_notification_preferences::preferences_json.eq(&new_pref.preferences_json),
                    user_notification_preferences::updated_at.eq(chrono::Utc::now().naive_utc()),
                ))
                .execute(conn)
                .ok();
        });
    }

    Ok(Json(json!({ "success": true })))
}

#[derive(Debug, Deserialize)]
struct UnsubscribeBody {
    endpoint: Option<String>,
}

async fn unsubscribe_push(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<UnsubscribeBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let endpoint = match body.endpoint {
        Some(ref e) if !e.is_empty() => e.clone(),
        _ => return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Missing endpoint"})))),
    };

    connection::with_db(|conn| {
        use schema::push_subscriptions;
        diesel::delete(
            push_subscriptions::table.filter(push_subscriptions::endpoint.eq(&endpoint)),
        )
        .execute(conn)
        .ok();
    });

    let current_prefs = get_or_create_prefs(user.id);
    if current_prefs["channels"]["webPush"].as_bool() == Some(true) {
        let mut updated = current_prefs;
        if let Some(obj) = updated.as_object_mut() {
            if let Some(channels) = obj.get_mut("channels").and_then(|c| c.as_object_mut()) {
                channels.insert("webPush".into(), json!(false));
            }
        }
        let json_str = serde_json::to_string(&updated).unwrap_or_default();
        connection::with_db(|conn| {
            use schema::user_notification_preferences;
            let new_pref = models::NewNotificationPreference {
                user_id: user.id,
                preferences_json: json_str,
            };
            diesel::insert_into(user_notification_preferences::table)
                .values(&new_pref)
                .on_conflict(user_notification_preferences::user_id)
                .do_update()
                .set((
                    user_notification_preferences::preferences_json.eq(&new_pref.preferences_json),
                    user_notification_preferences::updated_at.eq(chrono::Utc::now().naive_utc()),
                ))
                .execute(conn)
                .ok();
        });
    }

    Ok(Json(json!({ "success": true })))
}

// ═════════════════════════════════════════════════════════════════════════════
// Server Environment
// ═════════════════════════════════════════════════════════════════════════════

async fn get_server_env() -> Json<Value> {
    Json(json!({ "platform": std::env::consts::OS }))
}
