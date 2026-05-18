//! Sessions repository — Diesel implementation

use diesel::prelude::*;
use crate::db::connection;
use crate::db::models;
use crate::db::schema::sessions;
use crate::db::repos::projects::ProjectsRepo;

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub session_id: String,
    pub provider: String,
    pub project_path: Option<String>,
    pub jsonl_path: Option<String>,
    pub custom_name: Option<String>,
    pub is_archived: bool,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

impl From<models::Session> for SessionRow {
    fn from(s: models::Session) -> Self {
        SessionRow {
            session_id: s.session_id,
            provider: s.provider,
            project_path: s.project_path,
            jsonl_path: s.jsonl_path,
            custom_name: s.custom_name,
            is_archived: s.is_archived,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "messageCount")]
    pub message_count: i64,
    #[serde(rename = "lastActivity")]
    pub last_activity: Option<chrono::NaiveDateTime>,
}

pub struct SessionsRepo;

impl SessionsRepo {
    pub fn upsert(
        session_id: &str,
        provider: &str,
        project_path: &str,
        custom_name: Option<&str>,
        jsonl_path: Option<&str>,
    ) -> String {
        connection::with_db(|conn| {
            use sessions::dsl;

            let normalized_path = crate::shared::utils::normalize_project_path(project_path);

            // Ensure the project exists first (foreign key)
            ProjectsRepo::create_project_path(&normalized_path, None);

            let new_session = models::NewSession {
                session_id: session_id.to_string(),
                provider: provider.to_string(),
                custom_name: custom_name.map(String::from),
                project_path: if normalized_path.is_empty() {
                    None
                } else {
                    Some(normalized_path)
                },
                jsonl_path: jsonl_path.map(String::from),
            };

            diesel::insert_into(sessions::table)
                .values(&new_session)
                .on_conflict(dsl::session_id)
                .do_update()
                .set((
                    dsl::provider.eq(provider.to_string()),
                    dsl::updated_at.eq(chrono::Utc::now().naive_utc()),
                    dsl::project_path.eq(new_session.project_path.clone()),
                    dsl::jsonl_path.eq(new_session.jsonl_path.clone()),
                    dsl::isArchived.eq(false),
                    dsl::custom_name.eq(new_session.custom_name.clone()),
                ))
                .execute(conn)
                .expect("Failed to upsert session");

            session_id.to_string()
        })
    }

    pub fn get_by_id(session_id: &str) -> Option<SessionRow> {
        connection::with_db(|conn| {
            use sessions::dsl;
            dsl::sessions
                .filter(dsl::session_id.eq(session_id))
                .order(dsl::updated_at.desc())
                .first::<models::Session>(conn)
                .ok()
                .map(SessionRow::from)
        })
    }

    pub fn update_custom_name(session_id: &str, custom_name: &str) {
        connection::with_db(|conn| {
            use sessions::dsl;
            diesel::update(dsl::sessions.filter(dsl::session_id.eq(session_id)))
                .set((
                    dsl::custom_name.eq(Some(custom_name.to_string())),
                    dsl::updated_at.eq(chrono::Utc::now().naive_utc()),
                ))
                .execute(conn)
                .ok();
        });
    }

    pub fn list_sessions(project_path: Option<&str>) -> Vec<SessionRow> {
        connection::with_db(|conn| {
            use sessions::dsl;
            let query = dsl::sessions
                .filter(dsl::isArchived.eq(false))
                .order(dsl::updated_at.desc());
            let rows: Vec<SessionRow> = query
                .load::<models::Session>(conn)
                .unwrap_or_default()
                .into_iter()
                .map(SessionRow::from)
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

    pub fn list_sessions_paginated(
        project_path: &str,
        limit: i64,
        offset: i64,
    ) -> (Vec<SessionRow>, bool, i64) {
        connection::with_db(|conn| {
            use sessions::dsl;
            let normalized = crate::shared::utils::normalize_project_path(project_path);

            let total: i64 = dsl::sessions
                .filter(dsl::project_path.eq(&normalized))
                .filter(dsl::isArchived.eq(false))
                .select(diesel::dsl::count(dsl::session_id))
                .first(conn)
                .unwrap_or(0);

            let rows: Vec<SessionRow> = dsl::sessions
                .filter(dsl::project_path.eq(&normalized))
                .filter(dsl::isArchived.eq(false))
                .order(dsl::updated_at.desc())
                .limit(limit)
                .offset(offset)
                .load::<models::Session>(conn)
                .unwrap_or_default()
                .into_iter()
                .map(SessionRow::from)
                .collect();

            let has_more = (offset + limit) < total;
            (rows, has_more, total)
        })
    }

    pub fn list_archived_sessions() -> Vec<SessionRow> {
        connection::with_db(|conn| {
            use sessions::dsl;
            dsl::sessions
                .filter(dsl::isArchived.eq(true))
                .order(dsl::updated_at.desc())
                .load::<models::Session>(conn)
                .unwrap_or_default()
                .into_iter()
                .map(SessionRow::from)
                .collect()
        })
    }

    pub fn archive(session_id: &str) {
        connection::with_db(|conn| {
            use sessions::dsl;
            diesel::update(dsl::sessions.filter(dsl::session_id.eq(session_id)))
                .set((
                    dsl::isArchived.eq(true),
                    dsl::updated_at.eq(chrono::Utc::now().naive_utc()),
                ))
                .execute(conn)
                .ok();
        });
    }

    pub fn restore(session_id: &str) {
        connection::with_db(|conn| {
            use sessions::dsl;
            diesel::update(dsl::sessions.filter(dsl::session_id.eq(session_id)))
                .set((
                    dsl::isArchived.eq(false),
                    dsl::updated_at.eq(chrono::Utc::now().naive_utc()),
                ))
                .execute(conn)
                .ok();
        });
    }

    pub fn delete_by_id(session_id: &str) {
        connection::with_db(|conn| {
            use sessions::dsl;
            diesel::delete(dsl::sessions.filter(dsl::session_id.eq(session_id)))
                .execute(conn)
                .ok();
        });
    }

    pub fn get_session_summaries(project_path: &str) -> Vec<SessionSummary> {
        connection::with_db(|conn| {
            use sessions::dsl;
            let normalized = crate::shared::utils::normalize_project_path(project_path);

            dsl::sessions
                .filter(dsl::project_path.eq(&normalized))
                .filter(dsl::isArchived.eq(false))
                .order(dsl::updated_at.desc())
                .select((dsl::session_id, dsl::custom_name, dsl::updated_at))
                .load::<(String, Option<String>, Option<chrono::NaiveDateTime>)>(conn)
                .unwrap_or_default()
                .into_iter()
                .map(|(id, summary, last_activity)| SessionSummary {
                    id,
                    summary,
                    message_count: 0,
                    last_activity,
                })
                .collect()
        })
    }

    pub fn count_by_project_path(project_path: &str) -> i64 {
        connection::with_db(|conn| {
            use sessions::dsl;
            let normalized = crate::shared::utils::normalize_project_path(project_path);
            dsl::sessions
                .filter(dsl::project_path.eq(&normalized))
                .filter(dsl::isArchived.eq(false))
                .select(diesel::dsl::count(dsl::session_id))
                .first(conn)
                .unwrap_or(0)
        })
    }
}
