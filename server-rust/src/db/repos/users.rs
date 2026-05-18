//! Users repository — Diesel implementation

use diesel::prelude::*;
use crate::db::connection;
use crate::db::models;
use crate::db::schema::users;

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub last_login: Option<chrono::NaiveDateTime>,
    pub is_active: bool,
    pub git_name: Option<String>,
    pub git_email: Option<String>,
    pub has_completed_onboarding: bool,
}

#[derive(Debug, Clone)]
pub struct UserPublicRow {
    pub id: i64,
    pub username: String,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub last_login: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone)]
pub struct UserGitConfig {
    pub git_name: Option<String>,
    pub git_email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateUserResult {
    pub id: i64,
    pub username: String,
}

impl From<models::User> for UserRow {
    fn from(u: models::User) -> Self {
        UserRow {
            id: u.id,
            username: u.username,
            password_hash: u.password_hash,
            created_at: u.created_at,
            last_login: u.last_login,
            is_active: u.is_active,
            git_name: u.git_name,
            git_email: u.git_email,
            has_completed_onboarding: u.has_completed_onboarding,
        }
    }
}

pub struct UserRepo;

impl UserRepo {
    pub fn has_users() -> bool {
        connection::with_db(|conn| {
            use users::dsl;
            dsl::users
                .select(diesel::dsl::count(dsl::id))
                .first::<i64>(conn)
                .unwrap_or(0) > 0
        })
    }

    pub fn create_user(username: &str, password_hash: &str) -> CreateUserResult {
        connection::with_db(|conn| {
            use users::dsl;

            let new_user = models::NewUser {
                username: username.to_string(),
                password_hash: password_hash.to_string(),
            };

            let id: i64 = diesel::insert_into(users::table)
                .values(&new_user)
                .returning(users::id)
                .get_result(conn)
                .expect("Failed to create user");

            CreateUserResult {
                id,
                username: username.to_string(),
            }
        })
    }

    pub fn get_user_by_username(username: &str) -> Option<UserRow> {
        connection::with_db(|conn| {
            use users::dsl;
            dsl::users
                .filter(dsl::username.eq(username))
                .filter(dsl::is_active.eq(true))
                .first::<models::User>(conn)
                .ok()
                .map(UserRow::from)
        })
    }

    pub fn update_last_login(user_id: i64) {
        connection::with_db(|conn| {
            use users::dsl;
            let now = chrono::Utc::now().naive_utc();
            diesel::update(dsl::users.filter(dsl::id.eq(user_id)))
                .set(dsl::last_login.eq(Some(now)))
                .execute(conn)
                .ok();
        });
    }

    pub fn get_user_by_id(user_id: i64) -> Option<UserPublicRow> {
        connection::with_db(|conn| {
            use users::dsl;
            dsl::users
                .filter(dsl::id.eq(user_id))
                .filter(dsl::is_active.eq(true))
                .select((dsl::id, dsl::username, dsl::created_at, dsl::last_login))
                .first::<(i64, String, Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>)>(conn)
                .ok()
                .map(|(id, username, created_at, last_login)| UserPublicRow {
                    id,
                    username,
                    created_at,
                    last_login,
                })
        })
    }

    pub fn get_first_user() -> Option<UserPublicRow> {
        connection::with_db(|conn| {
            use users::dsl;
            dsl::users
                .filter(dsl::is_active.eq(true))
                .select((dsl::id, dsl::username, dsl::created_at, dsl::last_login))
                .first::<(i64, String, Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>)>(conn)
                .ok()
                .map(|(id, username, created_at, last_login)| UserPublicRow {
                    id,
                    username,
                    created_at,
                    last_login,
                })
        })
    }

    pub fn update_git_config(user_id: i64, git_name: &str, git_email: &str) {
        connection::with_db(|conn| {
            use users::dsl;
            diesel::update(dsl::users.filter(dsl::id.eq(user_id)))
                .set((
                    dsl::git_name.eq(Some(git_name.to_string())),
                    dsl::git_email.eq(Some(git_email.to_string())),
                ))
                .execute(conn)
                .ok();
        });
    }

    pub fn get_git_config(user_id: i64) -> Option<UserGitConfig> {
        connection::with_db(|conn| {
            use users::dsl;
            dsl::users
                .filter(dsl::id.eq(user_id))
                .select((dsl::git_name, dsl::git_email))
                .first::<(Option<String>, Option<String>)>(conn)
                .ok()
                .map(|(git_name, git_email)| UserGitConfig { git_name, git_email })
        })
    }

    pub fn complete_onboarding(user_id: i64) {
        connection::with_db(|conn| {
            use users::dsl;
            diesel::update(dsl::users.filter(dsl::id.eq(user_id)))
                .set(dsl::has_completed_onboarding.eq(true))
                .execute(conn)
                .ok();
        });
    }

    pub fn has_completed_onboarding(user_id: i64) -> bool {
        connection::with_db(|conn| {
            use users::dsl;
            dsl::users
                .filter(dsl::id.eq(user_id))
                .select(dsl::has_completed_onboarding)
                .first::<bool>(conn)
                .unwrap_or(false)
        })
    }
}
