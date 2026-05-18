//! Auth middleware — mirrors server/middleware/auth.js
//!
//! JWT authentication, API key validation, WebSocket auth.

use crate::db::repos::app_config::AppConfigRepo;
use crate::db::repos::users::UserRepo;
use crate::shared::config;
use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::LazyLock;

// ── JWT Claims ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub userId: i64,
    pub username: String,
    pub exp: usize,
    pub iat: usize,
}

// ── JWT Secret ───────────────────────────────────────────────────────────────

fn get_or_create_jwt_secret() -> String {
    if let Ok(secret) = std::env::var("JWT_SECRET") {
        if !secret.is_empty() {
            return secret;
        }
    }

    // Try app_config table (needs DB, which may not be ready at import time)
    if let Some(secret) = AppConfigRepo::get_jwt_secret() {
        return secret;
    }

    // Generate and persist
    let secret = uuid::Uuid::new_v4().to_string();
    AppConfigRepo::set_jwt_secret(&secret);
    secret
}

// ── Token Helpers ────────────────────────────────────────────────────────────

/// Generate a JWT token for a user (7-day expiry)
pub fn generate_token(user_id: i64, username: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;

    let claims = Claims {
        userId: user_id,
        username: username.to_string(),
        iat: now,
        exp: now + 7 * 24 * 3600, // 7 days
    };

    let secret = get_or_create_jwt_secret();
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("Failed to generate JWT")
}

/// Verify a JWT token
fn verify_token(token: &str) -> Option<Claims> {
    let secret = get_or_create_jwt_secret();
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .ok()
}

// ── Auth User ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
}

// ── Middleware: API Key Validation ───────────────────────────────────────────

/// API key validation middleware (if API_KEY env var is configured)
pub async fn validate_api_key(req: Request, next: Next) -> Result<Response, StatusCode> {
    if let Ok(api_key) = std::env::var("API_KEY") {
        if api_key.is_empty() {
            return Ok(next.run(req).await);
        }

        let provided = req
            .headers()
            .get("x-api-key")
            .and_then(|v| v.to_str().ok());

        if provided != Some(&api_key) {
            let body = Json(json!({"error": "Invalid API key"}));
            return Ok((StatusCode::UNAUTHORIZED, body).into_response());
        }
    }

    Ok(next.run(req).await)
}

// ── Middleware: JWT Token Authentication ─────────────────────────────────────

/// JWT authentication middleware
pub async fn authenticate_token(mut req: Request, next: Next) -> Response {
    // Platform mode: use first database user
    if config::IS_PLATFORM {
        match UserRepo::get_first_user() {
            Some(user) => {
                req.extensions_mut().insert(AuthUser {
                    id: user.id,
                    username: user.username,
                });
                return next.run(req).await;
            }
            None => {
                let body = Json(json!({"error": "Platform mode: No user found in database"}));
                return (StatusCode::INTERNAL_SERVER_ERROR, body).into_response();
            }
        }
    }

    // Normal JWT validation
    let token = extract_token(&req);

    let Some(token) = token else {
        let body = Json(json!({"error": "Access denied. No token provided."}));
        return (StatusCode::UNAUTHORIZED, body).into_response();
    };

    match verify_token(&token) {
        Some(claims) => {
            // Verify user still exists and is active
            if UserRepo::get_user_by_id(claims.userId).is_none() {
                let body = Json(json!({"error": "Invalid token. User not found."}));
                return (StatusCode::UNAUTHORIZED, body).into_response();
            }

            req.extensions_mut().insert(AuthUser {
                id: claims.userId,
                username: claims.username.clone(),
            });

            let mut response = next.run(req).await;

            // Auto-refresh: if token is past halfway through its lifetime
            if claims.exp > claims.iat {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as usize;
                let half_life = (claims.exp - claims.iat) / 2;
                if now > claims.iat + half_life {
                    let new_token = generate_token(claims.userId, &claims.username);
                    response
                        .headers_mut()
                        .insert("X-Refreshed-Token", new_token.parse().unwrap());
                }
            }

            response
        }
        None => {
            let body = Json(json!({"error": "Invalid token"}));
            (StatusCode::FORBIDDEN, body).into_response()
        }
    }
}

// ── WebSocket Authentication ─────────────────────────────────────────────────

/// Authenticate a WebSocket connection via token string
pub fn authenticate_websocket(token: Option<&str>) -> Option<AuthUser> {
    // Platform mode: return first user
    if config::IS_PLATFORM {
        return UserRepo::get_first_user().map(|u| AuthUser {
            id: u.id,
            username: u.username,
        });
    }

    let token = token?;
    let claims = verify_token(token)?;

    // Verify user exists in database
    let user = UserRepo::get_user_by_id(claims.userId)?;

    Some(AuthUser {
        id: user.id,
        username: user.username,
    })
}

// ── Token Extraction ─────────────────────────────────────────────────────────

/// Extract Bearer token from Authorization header or query param
fn extract_token(req: &Request) -> Option<String> {
    // Authorization: Bearer <token>
    if let Some(auth_header) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }

    // Query param (for SSE endpoints)
    if let Some(query) = req.uri().query() {
        for (key, value) in query.split('&').filter_map(|p| p.split_once('=')) {
            if key == "token" {
                return Some(value.to_string());
            }
        }
    }

    None
}

/// Helper to extract the authenticated user from request extensions
pub fn get_auth_user(req: &axum::extract::Request) -> Option<&AuthUser> {
    req.extensions().get::<AuthUser>()
}
