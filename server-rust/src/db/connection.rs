//! Database connection — mirrors server/modules/database/connection.ts
//!
//! Singleton connection management, path resolution, directory creation,
//! and legacy database migration.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::db::schema::APP_CONFIG_TABLE_SCHEMA_SQL;

/// Thread-safe singleton connection
static DB_INSTANCE: std::sync::LazyLock<Mutex<Option<Connection>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

/// Resolve the database path from DATABASE_PATH env var or default
fn resolve_database_path() -> PathBuf {
    if let Ok(db_path) = std::env::var("DATABASE_PATH") {
        PathBuf::from(db_path)
    } else {
        default_database_path()
    }
}

/// Default database path: ~/.cloudcli/database.sqlite
fn default_database_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    home.join(".cloudcli").join("database.sqlite")
}

/// Ensure the parent directory of the database exists
fn ensure_database_directory(db_path: &Path) {
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).expect("Failed to create database directory");
            tracing::info!("Created database directory: {}", parent.display());
        }
    }
}

/// Open or return the shared database connection
pub fn get_connection() -> std::sync::MutexGuard<'static, Option<Connection>> {
    let mut guard = DB_INSTANCE.lock().expect("Database lock poisoned");

    if guard.is_none() {
        let db_path = resolve_database_path();
        ensure_database_directory(&db_path);

        tracing::info!("Opening database at {}", db_path.display());

        let conn = Connection::open(&db_path).expect("Failed to open database");

        // Enable WAL mode and foreign keys
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;"
        ).expect("Failed to set database pragmas");

        // app_config must exist immediately for auth middleware
        conn.execute_batch(APP_CONFIG_TABLE_SCHEMA_SQL)
            .expect("Failed to create app_config table");

        *guard = Some(conn);
    }

    guard
}

/// Get the resolved database path without opening
pub fn get_database_path() -> PathBuf {
    resolve_database_path()
}

/// Close connection (primarily for testing/shutdown)
pub fn close_connection() {
    let mut guard = DB_INSTANCE.lock().expect("Database lock poisoned");
    if let Some(conn) = guard.take() {
        conn.close().ok();
        tracing::info!("Database connection closed");
    }
}

/// Execute a closure with the database connection
pub fn with_connection<F, T>(f: F) -> T
where
    F: FnOnce(&Connection) -> T,
{
    let guard = get_connection();
    let conn = guard.as_ref().expect("Database not initialized");
    f(conn)
}

/// Execute a closure with a mutable database connection
pub fn with_connection_mut<F, T>(f: F) -> T
where
    F: FnOnce(&Connection) -> T,
{
    let guard = get_connection();
    let conn = guard.as_ref().expect("Database not initialized");
    f(conn)
}
