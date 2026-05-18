//! User routes — mirrors server/modules/users/users.routes.ts
//!
//! GET  /api/user/profile — returns the authenticated user's profile
//! PUT  /api/user/profile — updates git config for the authenticated user

use axum::{
    Extension,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::db::repos::users::UserRepo;

pub fn routes() -> Router {
    Router::new()
        .route("/profile", get(get_profile).put(update_profile))
        .route("/git-config", get(get_git_config).post(update_git_config))
        .route("/onboarding-status", get(onboarding_status))
        .route("/complete-onboarding", post(complete_onboarding))
}

#[derive(Debug, Deserialize)]
struct UpdateProfileRequest {
    #[serde(rename = "gitName")]
    git_name: Option<String>,
    #[serde(rename = "gitEmail")]
    git_email: Option<String>,
}

/// GET /api/user/profile — return public profile info for the authenticated user
async fn get_profile(
    Extension(user): Extension<AuthUser>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let profile = UserRepo::get_user_by_id(user.id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "User not found"})),
        )
    })?;

    let git_config = UserRepo::get_git_config(user.id);

    Ok(Json(json!({
        "id": profile.id,
        "username": profile.username,
        "createdAt": profile.created_at,
        "lastLogin": profile.last_login,
        "gitName": git_config.as_ref().and_then(|g| g.git_name.as_deref()),
        "gitEmail": git_config.as_ref().and_then(|g| g.git_email.as_deref()),
    })))
}

/// PUT /api/user/profile — update git config for the authenticated user
async fn update_profile(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<UpdateProfileRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let git_name = body.git_name.unwrap_or_default();
    let git_email = body.git_email.unwrap_or_default();

    UserRepo::update_git_config(user.id, &git_name, &git_email);

    Ok(Json(json!({
        "success": true,
        "message": "Profile updated successfully"
    })))
}

#[derive(Debug, Deserialize)]
struct GitConfigRequest {
    #[serde(rename = "gitName")]
    git_name: String,
    #[serde(rename = "gitEmail")]
    git_email: String,
}

/// GET /api/user/git-config — get git config for the authenticated user
async fn get_git_config(
    Extension(user): Extension<AuthUser>,
) -> Json<Value> {
    let git_config = UserRepo::get_git_config(user.id);
    Json(json!({
        "gitName": git_config.as_ref().and_then(|g| g.git_name.as_deref()).unwrap_or(""),
        "gitEmail": git_config.as_ref().and_then(|g| g.git_email.as_deref()).unwrap_or("")
    }))
}

/// POST /api/user/git-config — update git config for the authenticated user
async fn update_git_config(
    Extension(user): Extension<AuthUser>,
    Json(body): Json<GitConfigRequest>,
) -> Json<Value> {
    UserRepo::update_git_config(user.id, &body.git_name, &body.git_email);
    Json(json!({
        "success": true,
        "message": "Git config updated successfully"
    }))
}

/// GET /api/user/onboarding-status — check if user has completed onboarding
async fn onboarding_status(
    Extension(user): Extension<AuthUser>,
) -> Json<Value> {
    let completed = UserRepo::has_completed_onboarding(user.id);
    Json(json!({
        "hasCompletedOnboarding": completed
    }))
}

/// POST /api/user/complete-onboarding — mark onboarding as complete
async fn complete_onboarding(
    Extension(user): Extension<AuthUser>,
) -> Json<Value> {
    UserRepo::complete_onboarding(user.id);
    Json(json!({
        "success": true,
        "hasCompletedOnboarding": true
    }))
}
