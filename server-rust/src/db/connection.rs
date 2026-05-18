//! Database connection — Diesel r2d2 pool with multi-backend support.
//!
//! - Default: SQLite via r2d2 pool (DATABASE_URL or DATABASE_PATH or ~/.cloudcli/database.sqlite)
//! - MySQL: when DATABASE_URL starts with "mysql://"
//! - PostgreSQL: when DATABASE_URL starts with "postgres://" or "postgresql://"
//!
//! The repos use `with_db()` which dispatches to the active backend's pool.

use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use std::path::{Path, PathBuf};

// ── Backend detection ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DbBackend {
    Sqlite,
    Mysql,
    Postgres,
}

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

// ── Connection pool (SQLite default) ──────────────────────────────────────────

static SQLITE_POOL: std::sync::RwLock<Option<r2d2::Pool<ConnectionManager<diesel::SqliteConnection>>>> =
    std::sync::RwLock::new(None);

fn resolve_database_url() -> String {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        if !url.starts_with("mysql://") && !url.starts_with("postgres") {
            if url.starts_with("sqlite://") {
                return url;
            }
            return format!("sqlite://{}", url);
        }
        return url;
    }
    if let Ok(db_path) = std::env::var("DATABASE_PATH") {
        return format!("sqlite://{}", db_path);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let default_path = home.join(".cloudcli").join("database.sqlite");
    format!("sqlite://{}", default_path.display())
}

fn ensure_database_directory(db_path: &str) {
    let path_str = db_path.strip_prefix("sqlite://").unwrap_or(db_path);
    let path = Path::new(path_str);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).expect("Failed to create database directory");
        }
    }
}

pub fn get_database_path() -> PathBuf {
    let url = resolve_database_url();
    let path_str = url.strip_prefix("sqlite://").unwrap_or(&url);
    PathBuf::from(path_str)
}

/// Initialize the database pool (call once at startup)
pub fn init_pool() {
    let database_url = resolve_database_url();
    let backend = get_db_backend();

    match backend {
        DbBackend::Sqlite => {
            ensure_database_directory(&database_url);
            tracing::info!("Opening SQLite database via Diesel r2d2");
            let manager = ConnectionManager::<diesel::SqliteConnection>::new(&database_url);
            let pool = Pool::builder()
                .max_size(16)
                .build(manager)
                .expect("Failed to create SQLite database pool");

            let mut guard = SQLITE_POOL.write().expect("Pool lock poisoned");
            *guard = Some(pool);
            drop(guard);

            // Run initial setup
            with_db(|conn| {
                use diesel::sql_query;
                sql_query("PRAGMA journal_mode=WAL")
                    .execute(conn)
                    .expect("Failed to set WAL mode");
                sql_query("PRAGMA foreign_keys=ON")
                    .execute(conn)
                    .expect("Failed to enable foreign keys");
                sql_query(crate::db::schema::APP_CONFIG_TABLE_SCHEMA_SQL)
                    .execute(conn)
                    .expect("Failed to create app_config table");
            });
        }
        DbBackend::Mysql => {
            unimplemented!("MySQL backend is not yet fully wired; use SQLite");
        }
        DbBackend::Postgres => {
            unimplemented!("PostgreSQL backend is not yet fully wired; use SQLite");
        }
    }
}

/// Get a pooled SQLite connection
pub fn get_conn() -> PooledConnection<ConnectionManager<diesel::SqliteConnection>> {
    SQLITE_POOL
        .read()
        .expect("Pool lock poisoned")
        .as_ref()
        .expect("Database pool not initialized — call init_pool() first")
        .get()
        .expect("Failed to get connection from pool")
}

/// Run a closure with a Diesel SQLite connection
pub fn with_db<F, T>(f: F) -> T
where
    F: FnOnce(&mut diesel::SqliteConnection) -> T,
{
    let mut conn = get_conn();
    f(&mut conn)
}

/// Close / drop the pool (for graceful shutdown)
pub fn close_pool() {
    let mut guard = SQLITE_POOL.write().expect("Pool lock poisoned");
    *guard = None;
    tracing::info!("Database pool closed");
}
