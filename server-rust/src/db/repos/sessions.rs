//! Sessions repository — mirrors server/modules/database/repositories/sessions.db.ts

use rusqlite::params;
use crate::db::{connection, repos::projects::ProjectsRepo};

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub session_id: String,
    pub provider: String,
    pub project_path: Option<String>,
    pub jsonl_path: Option<String>,
    pub custom_name: Option<String>,
    pub is_archived: i32,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

pub struct SessionsRepo;

impl SessionsRepo {
    /// Upsert a session record
    pub fn upsert(
        session_id: &str,
        provider: &str,
        project_path: &str,
        custom_name: Option<&str>,
        jsonl_path: Option<&str>,
    ) -> String {
        connection::with_connection(|db| {
            let normalized_path = crate::shared::utils::normalize_project_path(project_path);

            // Ensure the project exists first (foreign key)
            ProjectsRepo::create_project_path(&normalized_path, None);

            db.execute(
                "INSERT INTO sessions (session_id, provider, custom_name, project_path, jsonl_path, isArchived)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)
                 ON CONFLICT(session_id) DO UPDATE SET
                   provider = excluded.provider,
                   updated_at = CURRENT_TIMESTAMP,
                   project_path = excluded.project_path,
                   jsonl_path = excluded.jsonl_path,
                   isArchived = 0,
                   custom_name = COALESCE(excluded.custom_name, sessions.custom_name)",
                params![session_id, provider, custom_name, normalized_path, jsonl_path],
            )
            .expect("Failed to upsert session");
            session_id.to_string()
        })
    }

    /// Get a session by ID
    pub fn get_by_id(session_id: &str) -> Option<SessionRow> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT session_id, provider, project_path, jsonl_path, custom_name, isArchived, created_at, updated_at
                 FROM sessions WHERE session_id = ?1 ORDER BY updated_at DESC LIMIT 1",
                params![session_id],
                |row| {
                    Ok(SessionRow {
                        session_id: row.get(0)?,
                        provider: row.get(1)?,
                        project_path: row.get(2)?,
                        jsonl_path: row.get(3)?,
                        custom_name: row.get(4)?,
                        is_archived: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                },
            )
            .ok()
        })
    }

    /// Update session custom name
    pub fn update_custom_name(session_id: &str, custom_name: &str) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE sessions SET custom_name = ?1 WHERE session_id = ?2",
                params![custom_name, session_id],
            )
            .ok();
        });
    }

    /// List sessions (optionally filtered by project_path)
    pub fn list_sessions(project_path: Option<&str>) -> Vec<SessionRow> {
        connection::with_connection(|db| {
            let query = "SELECT session_id, provider, project_path, jsonl_path, custom_name, isArchived, created_at, updated_at
                         FROM sessions WHERE isArchived = 0 ORDER BY updated_at DESC";
            let mut stmt = db.prepare(query).expect("Failed to prepare query");
            let rows: Vec<SessionRow> = stmt
                .query_map([], |row| {
                    Ok(SessionRow {
                        session_id: row.get(0)?,
                        provider: row.get(1)?,
                        project_path: row.get(2)?,
                        jsonl_path: row.get(3)?,
                        custom_name: row.get(4)?,
                        is_archived: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })
                .expect("Failed to list sessions")
                .filter_map(|r| r.ok())
                .collect();

            // Filter by project_path if provided
            if let Some(path) = project_path {
                let normalized = crate::shared::utils::normalize_project_path(path);
                rows.into_iter()
                    .filter(|r| r.project_path.as_deref() == Some(&normalized))
                    .collect()
            } else {
                rows
            }
        })
    }

    /// Archive a session
    pub fn archive(session_id: &str) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE sessions SET isArchived = 1 WHERE session_id = ?1",
                params![session_id],
            )
            .ok();
        });
    }
}
