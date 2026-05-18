//! Database migrations — mirrors server/modules/database/migrations.ts

use rusqlite::Connection;

use crate::db::schema::{
    APP_CONFIG_TABLE_SCHEMA_SQL, LAST_SCANNED_AT_SQL, PROJECTS_TABLE_SCHEMA_SQL,
    PUSH_SUBSCRIPTIONS_TABLE_SCHEMA_SQL, SESSIONS_TABLE_SCHEMA_SQL,
    USER_NOTIFICATION_PREFERENCES_TABLE_SCHEMA_SQL, VAPID_KEYS_TABLE_SCHEMA_SQL,
};

/// SQL expression for generating a UUID v4 in SQLite
const SQLITE_UUID: &str = "lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(6)))";

/// Add a column to a table if it doesn't already exist
fn add_column_if_not_exists(
    db: &Connection,
    table: &str,
    column_names: &[String],
    column_name: &str,
    column_type: &str,
) {
    if !column_names.iter().any(|c| c == column_name) {
        tracing::info!("Running migration: Adding {column_name} column to {table} table");
        db.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column_name} {column_type}"),
            [],
        )
        .expect("Migration failed");
    }
}

fn table_exists(db: &Connection, table_name: &str) -> bool {
    db.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table_name],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

fn get_table_columns(db: &Connection, table: &str) -> Vec<String> {
    let mut stmt = db
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("Failed to get table info");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("Failed to read column names")
        .filter_map(|r| r.ok())
        .collect()
}

fn migrate_users_table(db: &Connection) {
    let columns = get_table_columns(db, "users");
    add_column_if_not_exists(db, "users", &columns, "git_name", "TEXT");
    add_column_if_not_exists(db, "users", &columns, "git_email", "TEXT");
    add_column_if_not_exists(
        db,
        "users",
        &columns,
        "has_completed_onboarding",
        "BOOLEAN DEFAULT 0",
    );
}

fn migrate_legacy_session_names(db: &Connection) {
    if !table_exists(db, "session_names") {
        return;
    }

    if table_exists(db, "sessions") {
        tracing::info!("Running migration: Merging session_names into sessions");
        db.execute_batch(
            "INSERT INTO sessions (session_id, provider, custom_name, created_at, updated_at)
             SELECT
               session_id,
               COALESCE(provider, 'claude'),
               custom_name,
               COALESCE(created_at, CURRENT_TIMESTAMP),
               COALESCE(updated_at, CURRENT_TIMESTAMP)
             FROM session_names
             WHERE true
             ON CONFLICT(session_id) DO UPDATE SET
               provider = excluded.provider,
               custom_name = COALESCE(excluded.custom_name, sessions.custom_name),
               created_at = COALESCE(sessions.created_at, excluded.created_at),
               updated_at = COALESCE(excluded.updated_at, sessions.updated_at);
             DROP TABLE session_names;",
        )
        .expect("Failed to migrate session_names");
    } else {
        tracing::info!("Running migration: Renaming session_names table to sessions");
        db.execute_batch("ALTER TABLE session_names RENAME TO sessions;")
            .expect("Failed to rename session_names");
    }
}

fn rebuild_projects_table(db: &Connection) {
    if !table_exists(db, "projects") {
        db.execute_batch(PROJECTS_TABLE_SCHEMA_SQL)
            .expect("Failed to create projects table");
        return;
    }

    let columns = get_table_columns(db, "projects");

    // Check if already has project_id primary key
    let has_pk: bool = db
        .prepare("SELECT COUNT(*) FROM pragma_table_info('projects') WHERE name='project_id' AND pk=1")
        .and_then(|mut s| s.query_row([], |r| r.get::<_, i64>(0)))
        .map(|c| c > 0)
        .unwrap_or(false);

    if has_pk {
        add_column_if_not_exists(db, "projects", &columns, "custom_project_name", "TEXT DEFAULT NULL");
        add_column_if_not_exists(db, "projects", &columns, "isStarred", "BOOLEAN DEFAULT 0");
        add_column_if_not_exists(db, "projects", &columns, "isArchived", "BOOLEAN DEFAULT 0");
        db.execute(
            &format!("UPDATE projects SET project_id = {SQLITE_UUID} WHERE project_id IS NULL OR trim(project_id) = ''"),
            [],
        ).expect("Failed to fix null project_ids");
        return;
    }

    tracing::info!("Running migration: Rebuilding projects table with project_id primary key");

    // Rebuild via new table
    db.execute_batch("PRAGMA foreign_keys = OFF;").ok();
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects_new (
            project_id TEXT PRIMARY KEY NOT NULL,
            project_path TEXT NOT NULL UNIQUE,
            custom_project_name TEXT DEFAULT NULL,
            isStarred BOOLEAN DEFAULT 0,
            isArchived BOOLEAN DEFAULT 0
        );
        INSERT INTO projects_new (project_id, project_path, custom_project_name, isStarred, isArchived)
        SELECT
          COALESCE(NULLIF(TRIM(project_id), ''), lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(6)))),
          project_path,
          custom_project_name,
          COALESCE(isStarred, 0),
          COALESCE(isArchived, 0)
        FROM projects
        WHERE project_path IS NOT NULL AND trim(project_path) <> '';
        DROP TABLE projects;
        ALTER TABLE projects_new RENAME TO projects;
        PRAGMA foreign_keys = ON;",
    )
    .expect("Failed to rebuild projects table");
}

fn rebuild_sessions_table(db: &Connection) {
    if !table_exists(db, "sessions") {
        db.execute_batch(SESSIONS_TABLE_SCHEMA_SQL)
            .expect("Failed to create sessions table");
        return;
    }

    let columns = get_table_columns(db, "sessions");
    let has_project_path = columns.iter().any(|c| c == "project_path");
    let has_provider = columns.iter().any(|c| c == "provider");

    if has_project_path && has_provider {
        // Add missing columns
        add_column_if_not_exists(db, "sessions", &columns, "jsonl_path", "TEXT");
        add_column_if_not_exists(db, "sessions", &columns, "isArchived", "BOOLEAN DEFAULT 0");
        add_column_if_not_exists(db, "sessions", &columns, "created_at", "DATETIME DEFAULT CURRENT_TIMESTAMP");
        add_column_if_not_exists(db, "sessions", &columns, "updated_at", "DATETIME DEFAULT CURRENT_TIMESTAMP");
        return;
    }

    tracing::info!("Running migration: Rebuilding sessions table");
    db.execute_batch("PRAGMA foreign_keys = OFF;").ok();
    // Simple rebuild
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions_new (
            session_id TEXT NOT NULL,
            provider TEXT NOT NULL DEFAULT 'claude',
            custom_name TEXT,
            project_path TEXT,
            jsonl_path TEXT,
            isArchived BOOLEAN DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (session_id)
        );
        INSERT INTO sessions_new (session_id, provider, custom_name, project_path)
        SELECT session_id, COALESCE(provider, 'claude'), custom_name, project_path
        FROM sessions WHERE session_id IS NOT NULL AND trim(session_id) <> '';
        DROP TABLE sessions;
        ALTER TABLE sessions_new RENAME TO sessions;
        PRAGMA foreign_keys = ON;",
    )
    .expect("Failed to rebuild sessions table");
}

fn ensure_projects_for_session_paths(db: &Connection) {
    if !table_exists(db, "sessions") {
        return;
    }

    db.execute(
        &format!(
            "INSERT INTO projects (project_id, project_path, custom_project_name, isStarred, isArchived)
             SELECT {SQLITE_UUID}, project_path, NULL, 0, 0
             FROM sessions
             WHERE project_path IS NOT NULL AND trim(project_path) <> ''
             ON CONFLICT(project_path) DO NOTHING"
        ),
        [],
    )
    .ok();
}

fn create_indexes(db: &Connection) {
    let indexes = [
        "CREATE INDEX IF NOT EXISTS idx_session_ids_lookup ON sessions(session_id)",
        "CREATE INDEX IF NOT EXISTS idx_sessions_project_path ON sessions(project_path)",
        "CREATE INDEX IF NOT EXISTS idx_sessions_is_archived ON sessions(isArchived)",
        "CREATE INDEX IF NOT EXISTS idx_projects_is_starred ON projects(isStarred)",
        "CREATE INDEX IF NOT EXISTS idx_projects_is_archived ON projects(isArchived)",
    ];
    for idx in indexes {
        db.execute_batch(idx).ok();
    }

    // Drop legacy indexes
    let drop_indexes = [
        "DROP INDEX IF EXISTS idx_session_names_lookup",
        "DROP INDEX IF EXISTS idx_sessions_workspace_path",
        "DROP INDEX IF EXISTS idx_workspace_original_paths_is_starred",
        "DROP INDEX IF EXISTS idx_workspace_original_paths_workspace_id",
    ];
    for idx in drop_indexes {
        db.execute_batch(idx).ok();
    }
}

fn drop_legacy_tables(db: &Connection) {
    if table_exists(db, "workspace_original_paths") {
        tracing::info!("Running migration: Dropping legacy workspace_original_paths table");
        db.execute_batch("DROP TABLE workspace_original_paths").ok();
    }
}

/// Run all database migrations
pub fn run_migrations(db: &Connection) {
    migrate_users_table(db);
    db.execute_batch(APP_CONFIG_TABLE_SCHEMA_SQL).expect("app_config migration");
    db.execute_batch(USER_NOTIFICATION_PREFERENCES_TABLE_SCHEMA_SQL).ok();
    db.execute_batch(VAPID_KEYS_TABLE_SCHEMA_SQL).ok();
    db.execute_batch(PUSH_SUBSCRIPTIONS_TABLE_SCHEMA_SQL).ok();
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_push_subscriptions_user_id ON push_subscriptions(user_id)").ok();

    rebuild_projects_table(db);
    migrate_legacy_session_names(db);
    rebuild_sessions_table(db);
    ensure_projects_for_session_paths(db);
    create_indexes(db);
    drop_legacy_tables(db);

    db.execute_batch(LAST_SCANNED_AT_SQL).ok();

    tracing::info!("Database migrations completed successfully");
}

/// Initialize the full database schema
pub fn initialize_database(db: &Connection) {
    use crate::db::schema::INIT_SCHEMA_SQL;
    db.execute_batch(INIT_SCHEMA_SQL)
        .expect("Failed to initialize database schema");
    tracing::info!("Database schema applied");
    run_migrations(db);
}
