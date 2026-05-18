//! Database migrations — uses diesel_migrations for schema management.
//!
//! The initial migration (00000000000000_initial) creates all tables.
//! Incremental migrations (e.g. adding columns) should be added as new
//! migration directories with higher timestamps.

use diesel::prelude::*;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Run all pending Diesel migrations on a SQLite connection
pub fn run_migrations(conn: &mut diesel::SqliteConnection) {
    conn.run_pending_migrations(MIGRATIONS)
        .expect("Failed to run database migrations");
    tracing::info!("Database migrations completed successfully");
}

/// Initialize the database by running pending migrations.
/// Called after the connection pool is set up.
pub fn initialize_database() {
    use crate::db::connection;
    connection::with_db(|conn| {
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run database migrations");
    });
    tracing::info!("Database initialized successfully");
}
