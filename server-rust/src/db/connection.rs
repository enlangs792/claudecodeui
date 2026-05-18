//! Database connection — dual-backend: rusqlite (primary) + Diesel r2d2 (opt-in)
//!
//! - rusqlite is used by all existing repos and is always available
//! - Diesel pool is initialized when DATABASE_URL points to MySQL or PostgreSQL,
//!   or when the "diesel" feature is explicitly enabled for SQLite

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Database backend type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DbBackend {
    Sqlite,
    Mysql,
    Postgres,
}

/// Thread-safe singleton rusqlite connection (primary for SQLite)
static DB_INSTANCE: std::sync::LazyLock<Mutex<Option<Connection>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

static DB_BACKEND: std::sync::LazyLock<DbBackend> = std::sync::LazyLock::new(|| {
    let url = std::env::var("DATABASE_URL").unwrap_or_default();
    if url.starts_with("mysql://") {
        DbBackend::Mysql
    } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        DbBackend::Postgres
    } else {
        DbBackend::Sqlite
    }
});

pub fn get_db_backend() -> DbBackend {
    *DB_BACKEND
}

// ── Path resolution ────────────────────────────────────────────────────────────

fn resolve_database_path() -> PathBuf {
    if let Ok(db_path) = std::env::var("DATABASE_PATH") {
        return PathBuf::from(db_path);
    }
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.starts_with("mysql://") && !url.starts_with("postgres") {
            return PathBuf::from(&url);
        }
    }
    default_database_path()
}

fn default_database_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    home.join(".cloudcli").join("database.sqlite")
}

pub fn get_database_path() -> PathBuf {
    resolve_database_path()
}

fn ensure_database_directory(db_path: &Path) {
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).expect("Failed to create database directory");
        }
    }
}

// ── rusqlite connection (always available for SQLite mode) ─────────────────────

pub fn get_connection() -> std::sync::MutexGuard<'static, Option<Connection>> {
    let mut guard = DB_INSTANCE.lock().expect("Database lock poisoned");

    if guard.is_none() && *DB_BACKEND == DbBackend::Sqlite {
        let db_path = resolve_database_path();
        ensure_database_directory(&db_path);
        tracing::info!("Opening SQLite database at {}", db_path.display());

        let conn = Connection::open(&db_path).expect("Failed to open database");

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;"
        ).expect("Failed to set pragmas");

        // app_config must exist immediately for auth middleware
        conn.execute_batch(crate::db::schema::APP_CONFIG_TABLE_SCHEMA_SQL)
            .expect("Failed to create app_config table");

        *guard = Some(conn);
    }

    guard
}

pub fn close_connection() {
    let mut guard = DB_INSTANCE.lock().expect("Database lock poisoned");
    if let Some(conn) = guard.take() {
        conn.close().ok();
    }
}

pub fn with_connection<F, T>(f: F) -> T
where
    F: FnOnce(&Connection) -> T,
{
    let guard = get_connection();
    let conn = guard.as_ref().expect("Database not initialized");
    f(conn)
}

pub fn with_connection_mut<F, T>(f: F) -> T
where
    F: FnOnce(&Connection) -> T,
{
    let guard = get_connection();
    let conn = guard.as_ref().expect("Database not initialized");
    f(conn)
}
