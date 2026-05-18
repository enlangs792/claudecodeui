//! API Keys repository — mirrors server/modules/database/repositories/api-keys.ts

use rusqlite::params;
use crate::db::connection;

#[derive(Debug, Clone)]
pub struct ApiKeyRow {
    pub id: i64,
    pub user_id: i64,
    pub key_name: String,
    pub api_key: String,
    pub created_at: String,
    pub last_used: Option<String>,
    pub is_active: i32,
}

pub struct ApiKeysRepo;

impl ApiKeysRepo {
    /// Create a new API key
    pub fn create(user_id: i64, key_name: &str, api_key: &str) -> i64 {
        connection::with_connection(|db| {
            db.execute(
                "INSERT INTO api_keys (user_id, key_name, api_key) VALUES (?1, ?2, ?3)",
                params![user_id, key_name, api_key],
            )
            .expect("Failed to create API key");
            db.last_insert_rowid()
        })
    }

    /// Validate an API key and update last_used
    pub fn validate(api_key: &str) -> Option<i64> {
        connection::with_connection(|db| {
            let user_id: Option<i64> = db
                .query_row(
                    "SELECT user_id FROM api_keys WHERE api_key = ?1 AND is_active = 1",
                    params![api_key],
                    |row| row.get(0),
                )
                .ok();

            if let Some(uid) = user_id {
                db.execute(
                    "UPDATE api_keys SET last_used = CURRENT_TIMESTAMP WHERE api_key = ?1",
                    params![api_key],
                )
                .ok();
            }

            user_id
        })
    }

    /// List API keys for a user
    pub fn list_by_user(user_id: i64) -> Vec<ApiKeyRow> {
        connection::with_connection(|db| {
            let mut stmt = db
                .prepare(
                    "SELECT id, user_id, key_name, api_key, created_at, last_used, is_active
                     FROM api_keys WHERE user_id = ?1 AND is_active = 1 ORDER BY created_at DESC",
                )
                .expect("Failed to prepare query");
            stmt.query_map(params![user_id], |row| {
                Ok(ApiKeyRow {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    key_name: row.get(2)?,
                    api_key: row.get(3)?,
                    created_at: row.get(4)?,
                    last_used: row.get(5)?,
                    is_active: row.get(6)?,
                })
            })
            .expect("Failed to list API keys")
            .filter_map(|r| r.ok())
            .collect()
        })
    }

    /// Deactivate an API key
    pub fn deactivate(key_id: i64) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE api_keys SET is_active = 0 WHERE id = ?1",
                params![key_id],
            )
            .ok();
        });
    }
}
