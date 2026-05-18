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

/// Lightweight session summary for project listing
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "messageCount")]
    pub message_count: i64,
    #[serde(rename = "lastActivity")]
    pub last_activity: Option<String>,
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

    /// List sessions by project path with pagination
    pub fn list_sessions_paginated(
        project_path: &str,
        limit: i64,
        offset: i64,
    ) -> (Vec<SessionRow>, bool, i64) {
        connection::with_connection(|db| {
            let normalized = crate::shared::utils::normalize_project_path(project_path);

            // Count total
            let total: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM sessions WHERE project_path = ?1 AND isArchived = 0",
                    params![&normalized],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let mut stmt = db
                .prepare(
                    "SELECT session_id, provider, project_path, jsonl_path, custom_name, isArchived, created_at, updated_at
                     FROM sessions WHERE project_path = ?1 AND isArchived = 0
                     ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3",
                )
                .expect("Failed to prepare query");

            let rows: Vec<SessionRow> = stmt
                .query_map(params![&normalized, limit, offset], |row| {
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
                .expect("Failed to list paginated sessions")
                .filter_map(|r| r.ok())
                .collect();

            let has_more = (offset + limit) < total;
            (rows, has_more, total)
        })
    }

    /// List archived sessions
    pub fn list_archived_sessions() -> Vec<SessionRow> {
        connection::with_connection(|db| {
            let mut stmt = db
                .prepare(
                    "SELECT session_id, provider, project_path, jsonl_path, custom_name, isArchived, created_at, updated_at
                     FROM sessions WHERE isArchived = 1 ORDER BY updated_at DESC",
                )
                .expect("Failed to prepare query");
            stmt.query_map([], |row| {
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
            .expect("Failed to list archived sessions")
            .filter_map(|r| r.ok())
            .collect()
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

    /// Restore a session from archive
    pub fn restore(session_id: &str) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE sessions SET isArchived = 0 WHERE session_id = ?1",
                params![session_id],
            )
            .ok();
        });
    }

    /// Delete a session by ID
    pub fn delete_by_id(session_id: &str) {
        connection::with_connection(|db| {
            db.execute("DELETE FROM sessions WHERE session_id = ?1", params![session_id])
                .ok();
        });
    }

    /// Get session summaries for a project path (for ProjectListItem)
    pub fn get_session_summaries(project_path: &str) -> Vec<SessionSummary> {
        connection::with_connection(|db| {
            let normalized = crate::shared::utils::normalize_project_path(project_path);
            let mut stmt = db
                .prepare(
                    "SELECT session_id, custom_name, updated_at
                     FROM sessions WHERE project_path = ?1 AND isArchived = 0
                     ORDER BY updated_at DESC",
                )
                .expect("Failed to prepare query");

            stmt.query_map(params![&normalized], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    summary: row.get(1)?,
                    message_count: 0, // JSONL parsing would be expensive; frontend can lazy-load
                    last_activity: row.get(2)?,
                })
            })
            .expect("Failed to get session summaries")
            .filter_map(|r| r.ok())
            .collect()
        })
    }

    /// Count sessions for a project path
    pub fn count_by_project_path(project_path: &str) -> i64 {
        connection::with_connection(|db| {
            let normalized = crate::shared::utils::normalize_project_path(project_path);
            db.query_row(
                "SELECT COUNT(*) FROM sessions WHERE project_path = ?1 AND isArchived = 0",
                params![&normalized],
                |row| row.get(0),
            )
            .unwrap_or(0)
        })
    }
}
