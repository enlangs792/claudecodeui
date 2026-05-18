//! Database schema — mirrors server/modules/database/schema.ts
//!
//! Contains Diesel table! macro definitions for all tables,
//! plus SQL constants for legacy migration reference.

// ── Diesel table! macros ──────────────────────────────────────────────────────

diesel::table! {
    users (id) {
        id -> BigInt,
        username -> Text,
        password_hash -> Text,
        created_at -> Nullable<Timestamp>,
        last_login -> Nullable<Timestamp>,
        is_active -> Bool,
        git_name -> Nullable<Text>,
        git_email -> Nullable<Text>,
        has_completed_onboarding -> Bool,
    }
}

diesel::table! {
    api_keys (id) {
        id -> BigInt,
        user_id -> BigInt,
        key_name -> Text,
        api_key -> Text,
        created_at -> Nullable<Timestamp>,
        last_used -> Nullable<Timestamp>,
        is_active -> Bool,
    }
}

diesel::table! {
    user_credentials (id) {
        id -> BigInt,
        user_id -> BigInt,
        credential_name -> Text,
        credential_type -> Text,
        credential_value -> Text,
        description -> Nullable<Text>,
        created_at -> Nullable<Timestamp>,
        is_active -> Bool,
    }
}

diesel::table! {
    user_notification_preferences (user_id) {
        user_id -> BigInt,
        preferences_json -> Text,
        updated_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    vapid_keys (id) {
        id -> BigInt,
        public_key -> Text,
        private_key -> Text,
        created_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    push_subscriptions (id) {
        id -> BigInt,
        user_id -> BigInt,
        endpoint -> Text,
        keys_p256dh -> Text,
        keys_auth -> Text,
        created_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    projects (project_id) {
        project_id -> Text,
        project_path -> Text,
        custom_project_name -> Nullable<Text>,
        isStarred -> Bool,
        isArchived -> Bool,
    }
}

diesel::table! {
    sessions (session_id) {
        session_id -> Text,
        provider -> Text,
        custom_name -> Nullable<Text>,
        project_path -> Nullable<Text>,
        jsonl_path -> Nullable<Text>,
        isArchived -> Bool,
        created_at -> Nullable<Timestamp>,
        updated_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    scan_state (id) {
        id -> Integer,
        last_scanned_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    app_config (key) {
        key -> Text,
        value -> Text,
        created_at -> Nullable<Timestamp>,
    }
}

// Allow join relationships
diesel::joinable!(api_keys -> users (user_id));
diesel::joinable!(user_credentials -> users (user_id));
diesel::joinable!(user_notification_preferences -> users (user_id));
diesel::joinable!(push_subscriptions -> users (user_id));
diesel::joinable!(sessions -> projects (project_path));

// ── Per-table SQL constants (used by migrations.rs during transition) ─────

pub const APP_CONFIG_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
";

pub const LAST_SCANNED_AT_SQL: &str = "
CREATE TABLE IF NOT EXISTS scan_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  last_scanned_at TIMESTAMP NULL
);
";

pub const PROJECTS_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT PRIMARY KEY NOT NULL,
    project_path TEXT NOT NULL UNIQUE,
    custom_project_name TEXT DEFAULT NULL,
    isStarred INTEGER DEFAULT 0,
    isArchived INTEGER DEFAULT 0
);
";

pub const SESSIONS_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT NOT NULL,
    provider TEXT NOT NULL DEFAULT 'claude',
    custom_name TEXT,
    project_path TEXT,
    jsonl_path TEXT,
    isArchived INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (session_id),
    FOREIGN KEY (project_path) REFERENCES projects(project_path)
    ON DELETE SET NULL
    ON UPDATE CASCADE
);
";

pub const PUSH_SUBSCRIPTIONS_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS push_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    endpoint TEXT NOT NULL UNIQUE,
    keys_p256dh TEXT NOT NULL,
    keys_auth TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
";

pub const USER_NOTIFICATION_PREFERENCES_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS user_notification_preferences (
    user_id INTEGER PRIMARY KEY,
    preferences_json TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
";

pub const VAPID_KEYS_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS vapid_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    public_key TEXT NOT NULL,
    private_key TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
";

pub const USER_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_login DATETIME,
    is_active INTEGER DEFAULT 1,
    git_name TEXT,
    git_email TEXT,
    has_completed_onboarding INTEGER DEFAULT 0
);
";

pub const API_KEYS_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    key_name TEXT NOT NULL,
    api_key TEXT UNIQUE NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_used DATETIME,
    is_active INTEGER DEFAULT 1,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
";

pub const USER_CREDENTIALS_TABLE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS user_credentials (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    credential_name TEXT NOT NULL,
    credential_type TEXT NOT NULL,
    credential_value TEXT NOT NULL,
    description TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    is_active INTEGER DEFAULT 1,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
";

// ── Full init schema (for fresh databases) ───────────────────────────────────

pub const INIT_SCHEMA_SQL: &str = "
-- Initialize authentication database

-- Users
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_login DATETIME,
    is_active INTEGER DEFAULT 1,
    git_name TEXT,
    git_email TEXT,
    has_completed_onboarding INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_active ON users(is_active);

-- API Keys
CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    key_name TEXT NOT NULL,
    api_key TEXT UNIQUE NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    last_used DATETIME,
    is_active INTEGER DEFAULT 1,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_api_keys_key ON api_keys(api_key);
CREATE INDEX IF NOT EXISTS idx_api_keys_user_id ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_active ON api_keys(is_active);

-- User Credentials
CREATE TABLE IF NOT EXISTS user_credentials (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    credential_name TEXT NOT NULL,
    credential_type TEXT NOT NULL,
    credential_value TEXT NOT NULL,
    description TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    is_active INTEGER DEFAULT 1,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_user_credentials_user_id ON user_credentials(user_id);
CREATE INDEX IF NOT EXISTS idx_user_credentials_type ON user_credentials(credential_type);
CREATE INDEX IF NOT EXISTS idx_user_credentials_active ON user_credentials(is_active);

-- Notification Preferences
CREATE TABLE IF NOT EXISTS user_notification_preferences (
    user_id INTEGER PRIMARY KEY,
    preferences_json TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_user_notification_preferences_user_id ON user_notification_preferences(user_id);

-- VAPID Keys
CREATE TABLE IF NOT EXISTS vapid_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    public_key TEXT NOT NULL,
    private_key TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Push Subscriptions
CREATE TABLE IF NOT EXISTS push_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    endpoint TEXT NOT NULL UNIQUE,
    keys_p256dh TEXT NOT NULL,
    keys_auth TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_push_subscriptions_user_id ON push_subscriptions(user_id);

-- Projects
CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT PRIMARY KEY NOT NULL,
    project_path TEXT NOT NULL UNIQUE,
    custom_project_name TEXT DEFAULT NULL,
    isStarred INTEGER DEFAULT 0,
    isArchived INTEGER DEFAULT 0
);

-- Sessions
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT NOT NULL,
    provider TEXT NOT NULL DEFAULT 'claude',
    custom_name TEXT,
    project_path TEXT,
    jsonl_path TEXT,
    isArchived INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (session_id),
    FOREIGN KEY (project_path) REFERENCES projects(project_path)
    ON DELETE SET NULL
    ON UPDATE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_session_ids_lookup ON sessions(session_id);

-- Scan State
CREATE TABLE IF NOT EXISTS scan_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  last_scanned_at TIMESTAMP NULL
);

-- App Config
CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
";
