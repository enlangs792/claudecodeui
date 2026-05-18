//! Database integration tests

#[cfg(test)]
mod tests {
    use cloudcli_server::db::connection;
    use cloudcli_server::db::migrations;
    use cloudcli_server::db::schema::INIT_SCHEMA_SQL;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
        conn.execute_batch(INIT_SCHEMA_SQL)
            .expect("Failed to initialize schema");
        migrations::run_migrations(&conn);
        conn
    }

    #[test]
    fn test_schema_creates_all_tables() {
        let conn = setup_test_db();

        let tables = ["users", "api_keys", "user_credentials", "user_notification_preferences",
                      "vapid_keys", "push_subscriptions", "projects", "sessions",
                      "scan_state", "app_config"];

        for table in &tables {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            assert!(count > 0, "Table {table} should exist");
        }
    }

    #[test]
    fn test_users_table_schema() {
        let conn = setup_test_db();

        conn.execute(
            "INSERT INTO users (username, password_hash) VALUES (?1, ?2)",
            rusqlite::params!["testuser", "hash123"],
        )
        .expect("Failed to insert user");

        let username: String = conn
            .query_row("SELECT username FROM users WHERE id=1", [], |row| row.get(0))
            .expect("Failed to query user");
        assert_eq!(username, "testuser");
    }

    #[test]
    fn test_projects_table_schema() {
        let conn = setup_test_db();

        conn.execute(
            "INSERT INTO projects (project_id, project_path, custom_project_name) VALUES (?1, ?2, ?3)",
            rusqlite::params!["proj-001", "/home/user/test", "Test Project"],
        )
        .expect("Failed to insert project");

        let path: String = conn
            .query_row("SELECT project_path FROM projects WHERE project_id='proj-001'", [], |row| row.get(0))
            .expect("Failed to query project");
        assert_eq!(path, "/home/user/test");
    }

    #[test]
    fn test_sessions_table_schema() {
        let conn = setup_test_db();

        conn.execute(
            "INSERT INTO projects (project_id, project_path) VALUES (?1, ?2)",
            rusqlite::params!["proj-002", "/tmp/test"],
        ).expect("Failed to insert project");

        conn.execute(
            "INSERT INTO sessions (session_id, provider, project_path) VALUES (?1, ?2, ?3)",
            rusqlite::params!["sess-001", "claude", "/tmp/test"],
        )
        .expect("Failed to insert session");

        let provider: String = conn
            .query_row("SELECT provider FROM sessions WHERE session_id='sess-001'", [], |row| row.get(0))
            .expect("Failed to query session");
        assert_eq!(provider, "claude");
    }
}
