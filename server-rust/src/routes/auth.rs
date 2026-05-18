//! Auth routes — mirrors server/routes/auth.js
//!
//! POST /api/auth/register, login, refresh, logout

use axum::{extract::State, http::StatusCode, middleware, response::Json, routing::{get, post}, Router, Extension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::middleware::{generate_token, AuthUser};
use crate::db::repos::users::UserRepo;

pub fn routes() -> Router {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/status", get(status))
        .route(
            "/user",
            get(current_user)
                .layer(middleware::from_fn(crate::auth::middleware::authenticate_token)),
        )
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
    success: bool,
    token: String,
    user: UserInfo,
    #[serde(rename = "hasCompletedOnboarding")]
    has_completed_onboarding: bool,
}

#[derive(Debug, Serialize)]
struct UserInfo {
    id: i64,
    username: String,
}

async fn register(Json(body): Json<RegisterRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if body.username.trim().is_empty() || body.password.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Username and password are required"})),
        ));
    }

    // Check if user already exists
    if UserRepo::get_user_by_username(body.username.trim()).is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "Username already exists"})),
        ));
    }

    // Hash password
    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to process registration"})),
            )
        })?;

    let result = UserRepo::create_user(body.username.trim(), &password_hash);
    let token = generate_token(result.id, &result.username);

    Ok(Json(json!({
        "success": true,
        "token": token,
        "user": { "id": result.id, "username": result.username },
        "hasCompletedOnboarding": false
    })))
}

async fn login(Json(body): Json<LoginRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user = UserRepo::get_user_by_username(body.username.trim())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid username or password"})),
            )
        })?;

    // Verify password
    let valid = bcrypt::verify(&body.password, &user.password_hash).unwrap_or(false);
    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid username or password"})),
        ));
    }

    UserRepo::update_last_login(user.id);
    let token = generate_token(user.id, &user.username);

    Ok(Json(json!({
        "success": true,
        "token": token,
        "user": { "id": user.id, "username": user.username },
        "hasCompletedOnboarding": user.has_completed_onboarding == 1
    })))
}

async fn refresh(
    Extension(user): Extension<AuthUser>,
) -> Json<Value> {
    let token = generate_token(user.id, &user.username);
    Json(json!({
        "success": true,
        "token": token,
        "user": { "id": user.id, "username": user.username }
    }))
}

async fn logout() -> Json<Value> {
    Json(json!({"success": true, "message": "Logged out successfully"}))
}

/// GET /api/auth/status — check if setup is needed
async fn status() -> Json<Value> {
    let has_users = UserRepo::has_users();
    Json(json!({
        "needsSetup": !has_users,
        "isAuthenticated": false
    }))
}

/// GET /api/auth/user — return the current authenticated user
async fn current_user(
    Extension(auth_user): Extension<AuthUser>,
) -> Json<Value> {
    Json(json!({
        "user": {
            "id": auth_user.id,
            "username": auth_user.username
        }
    }))
}
