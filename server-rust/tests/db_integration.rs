//! Database integration tests — Diesel implementation

#[cfg(test)]
mod tests {
    use diesel::prelude::*;
    use cloudcli_server::db::migrations;
    use cloudcli_server::db::schema;

    fn setup_test_db() -> diesel::SqliteConnection {
        let mut conn = diesel::SqliteConnection::establish(":memory:")
            .expect("Failed to create in-memory database");
        diesel::sql_query("PRAGMA foreign_keys=ON")
            .execute(&mut conn)
            .expect("Failed to enable foreign keys");
        migrations::run_migrations(&mut conn);
        conn
    }

    #[test]
    fn test_users_table_schema() {
        let mut conn = setup_test_db();
        use schema::users;
        let new_user = cloudcli_server::db::models::NewUser {
            username: "testuser".to_string(),
            password_hash: "hash123".to_string(),
        };
        diesel::insert_into(users::table)
            .values(&new_user)
            .execute(&mut conn)
            .expect("Failed to insert user");
        let username: String = users::table
            .filter(users::id.eq(1))
            .select(users::username)
            .first(&mut conn)
            .expect("Failed to query user");
        assert_eq!(username, "testuser");
    }

    #[test]
    fn test_projects_table_schema() {
        let mut conn = setup_test_db();
        use schema::projects;
        let new_project = cloudcli_server::db::models::NewProject {
            project_id: "proj-001".to_string(),
            project_path: "/home/user/test".to_string(),
            custom_project_name: Some("Test Project".to_string()),
        };
        diesel::insert_into(projects::table)
            .values(&new_project)
            .execute(&mut conn)
            .expect("Failed to insert project");
        let path: String = projects::table
            .filter(projects::project_id.eq("proj-001"))
            .select(projects::project_path)
            .first(&mut conn)
            .expect("Failed to query project");
        assert_eq!(path, "/home/user/test");
    }

    #[test]
    fn test_sessions_table_schema() {
        let mut conn = setup_test_db();
        use schema::{projects, sessions};
        let new_project = cloudcli_server::db::models::NewProject {
            project_id: "proj-002".to_string(),
            project_path: "/tmp/test".to_string(),
            custom_project_name: None,
        };
        diesel::insert_into(projects::table)
            .values(&new_project)
            .execute(&mut conn)
            .expect("Failed to insert project");
        let new_session = cloudcli_server::db::models::NewSession {
            session_id: "sess-001".to_string(),
            provider: "claude".to_string(),
            custom_name: None,
            project_path: Some("/tmp/test".to_string()),
            jsonl_path: None,
        };
        diesel::insert_into(sessions::table)
            .values(&new_session)
            .execute(&mut conn)
            .expect("Failed to insert session");
        let provider: String = sessions::table
            .filter(sessions::session_id.eq("sess-001"))
            .select(sessions::provider)
            .first(&mut conn)
            .expect("Failed to query session");
        assert_eq!(provider, "claude");
    }
}
