//! Credentials repository — mirrors server/modules/database/repositories/credentials.ts

use rusqlite::params;
use crate::db::connection;
use crate::shared::types::{CreateCredentialResult, CredentialPublicRow};

pub struct CredentialsRepo;

impl CredentialsRepo {
    /// Create a new credential
    pub fn create(user_id: i64, name: &str, cred_type: &str, value: &str, description: Option<&str>) -> CreateCredentialResult {
        connection::with_connection(|db| {
            db.execute(
                "INSERT INTO user_credentials (user_id, credential_name, credential_type, credential_value, description)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![user_id, name, cred_type, value, description],
            )
            .expect("Failed to create credential");
            CreateCredentialResult {
                id: db.last_insert_rowid(),
                credential_name: name.to_string(),
                credential_type: cred_type.to_string(),
            }
        })
    }

    /// List credentials for a user (without secret values)
    pub fn list_by_user(user_id: i64) -> Vec<CredentialPublicRow> {
        connection::with_connection(|db| {
            let mut stmt = db
                .prepare(
                    "SELECT id, credential_name, credential_type, description, created_at, is_active
                     FROM user_credentials WHERE user_id = ?1 AND is_active = 1 ORDER BY created_at DESC",
                )
                .expect("Failed to prepare query");
            stmt.query_map(params![user_id], |row| {
                Ok(CredentialPublicRow {
                    id: row.get(0)?,
                    credential_name: row.get(1)?,
                    credential_type: row.get(2)?,
                    description: row.get(3)?,
                    created_at: row.get(4)?,
                    is_active: row.get(5)?,
                })
            })
            .expect("Failed to list credentials")
            .filter_map(|r| r.ok())
            .collect()
        })
    }

    /// Deactivate a credential
    pub fn deactivate(credential_id: i64) {
        connection::with_connection(|db| {
            db.execute(
                "UPDATE user_credentials SET is_active = 0 WHERE id = ?1",
                params![credential_id],
            )
            .ok();
        });
    }

    /// Get credential value by name (for internal use)
    pub fn get_value(user_id: i64, credential_name: &str) -> Option<String> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT credential_value FROM user_credentials WHERE user_id = ?1 AND credential_name = ?2 AND is_active = 1",
                params![user_id, credential_name],
                |row| row.get(0),
            )
            .ok()
        })
    }
}
