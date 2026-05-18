//! Projects repository — Diesel implementation

use diesel::prelude::*;
use crate::db::connection;
use crate::db::models;
use crate::db::schema::projects;
use crate::shared::types::{CreateProjectPathOutcome, CreateProjectPathResult, ProjectRepositoryRow};

fn to_repo_row(p: models::Project) -> ProjectRepositoryRow {
    ProjectRepositoryRow {
        project_id: p.project_id,
        project_path: p.project_path,
        custom_project_name: p.custom_project_name,
        is_starred: p.is_starred as i32,
        is_archived: p.is_archived as i32,
    }
}

pub struct ProjectsRepo;

impl ProjectsRepo {
    pub fn create_project_path(
        project_path: &str,
        custom_project_name: Option<&str>,
    ) -> CreateProjectPathResult {
        connection::with_db(|conn| {
            use projects::dsl;

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

            // Try inserting a new project first
            let insert_result = diesel::insert_into(projects::table)
                .values(&models::NewProject {
                    project_id: attempted_id.clone(),
                    project_path: normalized_path.clone(),
                    custom_project_name: Some(display_name.to_string()),
                })
                .returning((
                    dsl::project_id,
                    dsl::project_path,
                    dsl::custom_project_name,
                    dsl::isStarred,
                    dsl::isArchived,
                ))
                .get_result::<(String, String, Option<String>, bool, bool)>(conn);

            match insert_result {
                Ok((pid, pp, cpn, is_starred, is_archived)) => {
                    let row = ProjectRepositoryRow {
                        project_id: pid,
                        project_path: pp,
                        custom_project_name: cpn,
                        is_starred: is_starred as i32,
                        is_archived: is_archived as i32,
                    };
                    let is_new = row.project_id == attempted_id;
                    if !is_new {
                        // Was reactivated via conflict resolution
                        // We need to handle the ON CONFLICT DO UPDATE separately
                        diesel::update(dsl::projects.filter(dsl::project_path.eq(&normalized_path)))
                            .set(dsl::isArchived.eq(false))
                            .execute(conn)
                            .ok();
                    }
                    CreateProjectPathResult {
                        outcome: if is_new {
                            CreateProjectPathOutcome::Created
                        } else {
                            CreateProjectPathOutcome::ReactivatedArchived
                        },
                        project: Some(row),
                    }
                }
                Err(diesel::result::Error::NotFound) => {
                    // Conflicting active project
                    let existing = Self::get_project_path_internal(conn, &normalized_path);
                    CreateProjectPathResult {
                        outcome: CreateProjectPathOutcome::ActiveConflict,
                        project: existing,
                    }
                }
                Err(_) => {
                    let existing = Self::get_project_path_internal(conn, &normalized_path);
                    CreateProjectPathResult {
                        outcome: CreateProjectPathOutcome::ActiveConflict,
                        project: existing,
                    }
                }
            }
        })
    }

    fn get_project_path_internal(
        conn: &mut diesel::SqliteConnection,
        normalized_path: &str,
    ) -> Option<ProjectRepositoryRow> {
        use projects::dsl;
        dsl::projects
            .filter(dsl::project_path.eq(normalized_path))
            .select((
                dsl::project_id,
                dsl::project_path,
                dsl::custom_project_name,
                dsl::isStarred,
                dsl::isArchived,
            ))
            .first::<(String, String, Option<String>, bool, bool)>(conn)
            .ok()
            .map(|(pid, pp, cpn, is_starred, is_archived)| ProjectRepositoryRow {
                project_id: pid,
                project_path: pp,
                custom_project_name: cpn,
                is_starred: is_starred as i32,
                is_archived: is_archived as i32,
            })
    }

    pub fn get_project_path(project_path: &str) -> Option<ProjectRepositoryRow> {
        connection::with_db(|conn| {
            let normalized = crate::shared::utils::normalize_project_path(project_path);
            Self::get_project_path_internal(conn, &normalized)
        })
    }

    pub fn get_project_by_id(project_id: &str) -> Option<ProjectRepositoryRow> {
        connection::with_db(|conn| {
            use projects::dsl;
            dsl::projects
                .filter(dsl::project_id.eq(project_id))
                .select((
                    dsl::project_id,
                    dsl::project_path,
                    dsl::custom_project_name,
                    dsl::isStarred,
                    dsl::isArchived,
                ))
                .first::<(String, String, Option<String>, bool, bool)>(conn)
                .ok()
                .map(|(pid, pp, cpn, is_starred, is_archived)| ProjectRepositoryRow {
                    project_id: pid,
                    project_path: pp,
                    custom_project_name: cpn,
                    is_starred: is_starred as i32,
                    is_archived: is_archived as i32,
                })
        })
    }

    pub fn get_project_path_by_id(project_id: &str) -> Option<String> {
        connection::with_db(|conn| {
            use projects::dsl;
            dsl::projects
                .filter(dsl::project_id.eq(project_id))
                .select(dsl::project_path)
                .first::<String>(conn)
                .ok()
        })
    }

    pub fn list_projects() -> Vec<ProjectRepositoryRow> {
        connection::with_db(|conn| {
            use projects::dsl;
            dsl::projects
                .filter(dsl::isArchived.eq(false))
                .order((dsl::isStarred.desc(), dsl::custom_project_name.asc()))
                .select((
                    dsl::project_id,
                    dsl::project_path,
                    dsl::custom_project_name,
                    dsl::isStarred,
                    dsl::isArchived,
                ))
                .load::<(String, String, Option<String>, bool, bool)>(conn)
                .unwrap_or_default()
                .into_iter()
                .map(|(pid, pp, cpn, is_starred, is_archived)| ProjectRepositoryRow {
                    project_id: pid,
                    project_path: pp,
                    custom_project_name: cpn,
                    is_starred: is_starred as i32,
                    is_archived: is_archived as i32,
                })
                .collect()
        })
    }

    pub fn list_archived_projects() -> Vec<ProjectRepositoryRow> {
        connection::with_db(|conn| {
            use projects::dsl;
            dsl::projects
                .filter(dsl::isArchived.eq(true))
                .order(dsl::custom_project_name.asc())
                .select((
                    dsl::project_id,
                    dsl::project_path,
                    dsl::custom_project_name,
                    dsl::isStarred,
                    dsl::isArchived,
                ))
                .load::<(String, String, Option<String>, bool, bool)>(conn)
                .unwrap_or_default()
                .into_iter()
                .map(|(pid, pp, cpn, is_starred, is_archived)| ProjectRepositoryRow {
                    project_id: pid,
                    project_path: pp,
                    custom_project_name: cpn,
                    is_starred: is_starred as i32,
                    is_archived: is_archived as i32,
                })
                .collect()
        })
    }

    pub fn update_custom_name_by_id(project_id: &str, name: &str) -> bool {
        connection::with_db(|conn| {
            use projects::dsl;
            diesel::update(dsl::projects.filter(dsl::project_id.eq(project_id)))
                .set(dsl::custom_project_name.eq(Some(name.to_string())))
                .execute(conn)
                .map(|n| n > 0)
                .unwrap_or(false)
        })
    }

    pub fn update_star_by_id(project_id: &str, starred: bool) {
        connection::with_db(|conn| {
            use projects::dsl;
            diesel::update(dsl::projects.filter(dsl::project_id.eq(project_id)))
                .set(dsl::isStarred.eq(starred))
                .execute(conn)
                .ok();
        });
    }

    pub fn update_archive_by_id(project_id: &str, archived: bool) {
        connection::with_db(|conn| {
            use projects::dsl;
            diesel::update(dsl::projects.filter(dsl::project_id.eq(project_id)))
                .set(dsl::isArchived.eq(archived))
                .execute(conn)
                .ok();
        });
    }

    pub fn delete_by_id(project_id: &str) {
        connection::with_db(|conn| {
            use projects::dsl;
            diesel::delete(dsl::projects.filter(dsl::project_id.eq(project_id)))
                .execute(conn)
                .ok();
        });
    }

    pub fn migrate_legacy_stars(project_ids: &[String]) -> usize {
        connection::with_db(|conn| {
            use projects::dsl;
            let mut updated = 0usize;
            for pid in project_ids {
                if let Ok(n) = diesel::update(dsl::projects.filter(dsl::project_id.eq(pid)))
                    .set(dsl::isStarred.eq(true))
                    .execute(conn)
                {
                    updated += n;
                }
            }
            updated
        })
    }
}
