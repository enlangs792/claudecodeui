//! Projects repository — mirrors server/modules/database/repositories/projects.db.ts

use rusqlite::params;

use crate::db::connection;
use crate::shared::types::{CreateProjectPathOutcome, CreateProjectPathResult, ProjectRepositoryRow};

pub struct ProjectsRepo;

impl ProjectsRepo {
    /// Create or reactivate a project path
    pub fn create_project_path(
        project_path: &str,
        custom_project_name: Option<&str>,
    ) -> CreateProjectPathResult {
        connection::with_connection(|db| {
            let normalized_path = crate::shared::utils::normalize_project_path(project_path);
            let display_name = custom_project_name
                .map(|n| n.trim())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| {
                    std::path::Path::new(&normalized_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&normalized_path)
                });
            let attempted_id = uuid::Uuid::new_v4().to_string();

            // Try INSERT ... ON CONFLICT UPDATE (reactivate archived)
            let result: Option<ProjectRepositoryRow> = db
                .query_row(
                    "INSERT INTO projects (project_id, project_path, custom_project_name, isArchived)
                     VALUES (?1, ?2, ?3, 0)
                     ON CONFLICT(project_path) DO UPDATE SET isArchived = 0
                     WHERE projects.isArchived = 1
                     RETURNING project_id, project_path, custom_project_name, isStarred, isArchived",
                    params![attempted_id, normalized_path, display_name],
                    |row| {
                        Ok(ProjectRepositoryRow {
                            project_id: row.get(0)?,
                            project_path: row.get(1)?,
                            custom_project_name: row.get(2)?,
                            is_starred: row.get(3)?,
                            is_archived: row.get(4)?,
                        })
                    },
                )
                .ok();

            if let Some(row) = result {
                return CreateProjectPathResult {
                    outcome: if row.project_id == attempted_id {
                        CreateProjectPathOutcome::Created
                    } else {
                        CreateProjectPathOutcome::ReactivatedArchived
                    },
                    project: Some(row),
                };
            }

            // Conflicting active project
            let existing = Self::get_project_path(&normalized_path);
            CreateProjectPathResult {
                outcome: CreateProjectPathOutcome::ActiveConflict,
                project: existing,
            }
        })
    }

    /// Get project by path
    pub fn get_project_path(project_path: &str) -> Option<ProjectRepositoryRow> {
        connection::with_connection(|db| {
            let normalized = crate::shared::utils::normalize_project_path(project_path);
            db.query_row(
                "SELECT project_id, project_path, custom_project_name, isStarred, isArchived
                 FROM projects WHERE project_path = ?1",
                params![normalized],
                |row| {
                    Ok(ProjectRepositoryRow {
                        project_id: row.get(0)?,
                        project_path: row.get(1)?,
                        custom_project_name: row.get(2)?,
                        is_starred: row.get(3)?,
                        is_archived: row.get(4)?,
                    })
                },
            )
            .ok()
        })
    }

    /// Get project by ID
    pub fn get_project_by_id(project_id: &str) -> Option<ProjectRepositoryRow> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT project_id, project_path, custom_project_name, isStarred, isArchived
                 FROM projects WHERE project_id = ?1",
                params![project_id],
                |row| {
                    Ok(ProjectRepositoryRow {
                        project_id: row.get(0)?,
                        project_path: row.get(1)?,
                        custom_project_name: row.get(2)?,
                        is_starred: row.get(3)?,
                        is_archived: row.get(4)?,
                    })
                },
            )
            .ok()
        })
    }

    /// Resolve absolute project path from database project_id
    pub fn get_project_path_by_id(project_id: &str) -> Option<String> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT project_path FROM projects WHERE project_id = ?1",
                params![project_id],
                |row| row.get::<_, String>(0),
            )
            .ok()
        })
    }

    /// List all active (non-archived) projects
    pub fn list_projects() -> Vec<ProjectRepositoryRow> {
        connection::with_connection(|db| {
            let mut stmt = db
                .prepare(
                    "SELECT project_id, project_path, custom_project_name, isStarred, isArchived
                     FROM projects WHERE isArchived = 0 ORDER BY isStarred DESC, custom_project_name",
                )
                .expect("Failed to prepare query");
            stmt.query_map([], |row| {
                Ok(ProjectRepositoryRow {
                    project_id: row.get(0)?,
                    project_path: row.get(1)?,
                    custom_project_name: row.get(2)?,
                    is_starred: row.get(3)?,
                    is_archived: row.get(4)?,
                })
            })
            .expect("Failed to query projects")
            .filter_map(|r| r.ok())
            .collect()
        })
    }

    /// Toggle star on a project
    pub fn update_star_by_id(project_id: &str, starred: bool) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE projects SET isStarred = ?1 WHERE project_id = ?2",
                params![starred as i32, project_id],
            )
            .ok();
        });
    }

    /// Archive/unarchive a project
    pub fn update_archive_by_id(project_id: &str, archived: bool) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE projects SET isArchived = ?1 WHERE project_id = ?2",
                params![archived as i32, project_id],
            )
            .ok();
        });
    }

    /// Delete a project by ID
    pub fn delete_by_id(project_id: &str) {
        connection::with_connection(|db| {
            db.execute("DELETE FROM projects WHERE project_id = ?1", params![project_id])
                .ok();
        });
    }
}
