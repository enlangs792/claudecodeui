//! Users repository — mirrors server/modules/database/repositories/users.ts

use rusqlite::{params, Connection};

use crate::db::connection;

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub created_at: String,
    pub last_login: Option<String>,
    pub is_active: i32,
    pub git_name: Option<String>,
    pub git_email: Option<String>,
    pub has_completed_onboarding: i32,
}

#[derive(Debug, Clone)]
pub struct UserPublicRow {
    pub id: i64,
    pub username: String,
    pub created_at: String,
    pub last_login: Option<String>,
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

pub struct UserRepo;

impl UserRepo {
    /// Returns true if at least one user exists
    pub fn has_users() -> bool {
        connection::with_connection(|db| {
            let count: i64 = db
                .query_row("SELECT COUNT(*) as count FROM users", [], |row| row.get(0))
                .unwrap_or(0);
            count > 0
        })
    }

    /// Create a new user
    pub fn create_user(username: &str, password_hash: &str) -> CreateUserResult {
        connection::with_connection(|db| {
            db.execute(
                "INSERT INTO users (username, password_hash) VALUES (?1, ?2)",
                params![username, password_hash],
            )
            .expect("Failed to create user");
            let id = db.last_insert_rowid();
            CreateUserResult {
                id,
                username: username.to_string(),
            }
        })
    }

    /// Look up active user by username (includes password hash for auth)
    pub fn get_user_by_username(username: &str) -> Option<UserRow> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT id, username, password_hash, created_at, last_login, is_active, git_name, git_email, has_completed_onboarding
                 FROM users WHERE username = ?1 AND is_active = 1",
                params![username],
                |row| {
                    Ok(UserRow {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        created_at: row.get(3)?,
                        last_login: row.get(4)?,
                        is_active: row.get(5)?,
                        git_name: row.get(6)?,
                        git_email: row.get(7)?,
                        has_completed_onboarding: row.get(8)?,
                    })
                },
            )
            .ok()
        })
    }

    /// Update last_login timestamp
    pub fn update_last_login(user_id: i64) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE users SET last_login = CURRENT_TIMESTAMP WHERE id = ?1",
                params![user_id],
            )
            .ok();
        });
    }

    /// Get public user info by ID
    pub fn get_user_by_id(user_id: i64) -> Option<UserPublicRow> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT id, username, created_at, last_login FROM users WHERE id = ?1 AND is_active = 1",
                params![user_id],
                |row| {
                    Ok(UserPublicRow {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        created_at: row.get(2)?,
                        last_login: row.get(3)?,
                    })
                },
            )
            .ok()
        })
    }

    /// Get the first active user
    pub fn get_first_user() -> Option<UserPublicRow> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT id, username, created_at, last_login FROM users WHERE is_active = 1 LIMIT 1",
                [],
                |row| {
                    Ok(UserPublicRow {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        created_at: row.get(2)?,
                        last_login: row.get(3)?,
                    })
                },
            )
            .ok()
        })
    }

    /// Update git config for user
    pub fn update_git_config(user_id: i64, git_name: &str, git_email: &str) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE users SET git_name = ?1, git_email = ?2 WHERE id = ?3",
                params![git_name, git_email, user_id],
            )
            .ok();
        });
    }

    /// Get git config for user
    pub fn get_git_config(user_id: i64) -> Option<UserGitConfig> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT git_name, git_email FROM users WHERE id = ?1",
                params![user_id],
                |row| {
                    Ok(UserGitConfig {
                        git_name: row.get(0)?,
                        git_email: row.get(1)?,
                    })
                },
            )
            .ok()
        })
    }

    /// Mark onboarding as complete
    pub fn complete_onboarding(user_id: i64) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE users SET has_completed_onboarding = 1 WHERE id = ?1",
                params![user_id],
            )
            .ok();
        });
    }

    /// Check if user has completed onboarding
    pub fn has_completed_onboarding(user_id: i64) -> bool {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT has_completed_onboarding FROM users WHERE id = ?1",
                params![user_id],
                |row| row.get::<_, i32>(0),
            )
            .map(|v| v == 1)
            .unwrap_or(false)
        })
    }
}
