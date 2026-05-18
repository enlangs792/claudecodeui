//! Diesel ORM models — Queryable, Insertable, and domain structs for all tables.
//!
//! These models are backend-agnostic and work with SQLite, MySQL, and PostgreSQL.

use diesel::prelude::*;
use serde::{Deserialize, Serialize};

// ── Users ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::users)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub last_login: Option<chrono::NaiveDateTime>,
    pub is_active: bool,
    pub git_name: Option<String>,
    pub git_email: Option<String>,
    pub has_completed_onboarding: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::users)]
pub struct NewUser {
    pub username: String,
    pub password_hash: String,
}

#[derive(Debug, Clone, AsChangeset)]
#[diesel(table_name = crate::db::schema::users)]
pub struct UserChangeset {
    pub last_login: Option<Option<chrono::NaiveDateTime>>,
    pub git_name: Option<Option<String>>,
    pub git_email: Option<Option<String>>,
    pub has_completed_onboarding: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct UserPublicRow {
    pub id: i64,
    pub username: String,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub last_login: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone)]
pub struct UserGitConfig {
    pub git_name: Option<String>,
    pub git_email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateUserResult {
    pub id: i64,
    pub username: String,
}

// ── API Keys ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::api_keys)]
#[diesel(belongs_to(User))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ApiKey {
    pub id: i64,
    pub user_id: i64,
    pub key_name: String,
    pub api_key: String,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub last_used: Option<chrono::NaiveDateTime>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::api_keys)]
pub struct NewApiKey {
    pub user_id: i64,
    pub key_name: String,
    pub api_key: String,
}

#[derive(Debug, Clone)]
pub struct ApiKeyRow {
    pub id: i64,
    pub user_id: i64,
    pub key_name: String,
    pub api_key: String,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub last_used: Option<chrono::NaiveDateTime>,
    pub is_active: bool,
}

// ── User Credentials ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::user_credentials)]
#[diesel(belongs_to(User))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct UserCredential {
    pub id: i64,
    pub user_id: i64,
    pub credential_name: String,
    pub credential_type: String,
    pub credential_value: String,
    pub description: Option<String>,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::user_credentials)]
pub struct NewCredential {
    pub user_id: i64,
    pub credential_name: String,
    pub credential_type: String,
    pub credential_value: String,
    pub description: Option<String>,
}

// ── User Notification Preferences ─────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::user_notification_preferences)]
#[diesel(primary_key(user_id))]
#[diesel(belongs_to(User))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct UserNotificationPreference {
    pub user_id: i64,
    pub preferences_json: String,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::user_notification_preferences)]
pub struct NewNotificationPreference {
    pub user_id: i64,
    pub preferences_json: String,
}

// ── VAPID Keys ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::vapid_keys)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct VapidKey {
    pub id: i64,
    pub public_key: String,
    pub private_key: String,
    pub created_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::vapid_keys)]
pub struct NewVapidKey {
    pub public_key: String,
    pub private_key: String,
}

// ── Push Subscriptions ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::push_subscriptions)]
#[diesel(belongs_to(User))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct PushSubscription {
    pub id: i64,
    pub user_id: i64,
    pub endpoint: String,
    pub keys_p256dh: String,
    pub keys_auth: String,
    pub created_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::push_subscriptions)]
pub struct NewPushSubscription {
    pub user_id: i64,
    pub endpoint: String,
    pub keys_p256dh: String,
    pub keys_auth: String,
}

// ── Projects ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::projects)]
#[diesel(primary_key(project_id))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Project {
    pub project_id: String,
    pub project_path: String,
    pub custom_project_name: Option<String>,
    #[diesel(column_name = "isStarred")]
    pub is_starred: bool,
    #[diesel(column_name = "isArchived")]
    pub is_archived: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::projects)]
pub struct NewProject {
    pub project_id: String,
    pub project_path: String,
    pub custom_project_name: Option<String>,
}

/// Database row compatible with shared types (legacy ProjectRepositoryRow shape)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRepositoryRow {
    pub project_id: String,
    pub project_path: String,
    pub custom_project_name: Option<String>,
    #[serde(rename = "isStarred")]
    pub is_starred: bool,
    #[serde(rename = "isArchived")]
    pub is_archived: bool,
}

// ── Sessions ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::sessions)]
#[diesel(primary_key(session_id))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Session {
    pub session_id: String,
    pub provider: String,
    pub custom_name: Option<String>,
    pub project_path: Option<String>,
    pub jsonl_path: Option<String>,
    #[diesel(column_name = "isArchived")]
    pub is_archived: bool,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::sessions)]
pub struct NewSession {
    pub session_id: String,
    pub provider: String,
    pub custom_name: Option<String>,
    pub project_path: Option<String>,
    pub jsonl_path: Option<String>,
}

/// Lightweight session summary for project listing
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "messageCount")]
    pub message_count: i64,
    #[serde(rename = "lastActivity")]
    pub last_activity: Option<chrono::NaiveDateTime>,
}

// ── Scan State ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::scan_state)]
#[diesel(primary_key(id))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ScanState {
    pub id: i32,
    pub last_scanned_at: Option<chrono::NaiveDateTime>,
}

// ── App Config ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::app_config)]
#[diesel(primary_key(key))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct AppConfig {
    pub key: String,
    pub value: String,
    pub created_at: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::app_config)]
pub struct NewAppConfig {
    pub key: String,
    pub value: String,
}

// ── GitHub Tokens ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = crate::db::schema::github_tokens)]
#[diesel(primary_key(id))]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct GitHubToken {
    pub id: i32,
    pub user_id: i32,
    pub token: String,
    pub token_name: Option<String>,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub last_used: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::github_tokens)]
pub struct NewGitHubToken {
    pub user_id: i32,
    pub token: String,
    pub token_name: Option<String>,
}
