//! App Config repository — mirrors server/modules/database/repositories/app-config.ts

use rusqlite::params;
use crate::db::connection;

pub struct AppConfigRepo;

impl AppConfigRepo {
    /// Get a config value by key
    pub fn get(key: &str) -> Option<String> {
        connection::with_connection(|db| {
            db.query_row(
                "SELECT value FROM app_config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok()
        })
    }

    /// Set a config key-value pair
    pub fn set(key: &str, value: &str) {
        connection::with_connection(|db| {
            db.execute(
                "INSERT INTO app_config (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .ok();
        });
    }

    /// Delete a config key
    pub fn delete(key: &str) {
        connection::with_connection(|db| {
            db.execute("DELETE FROM app_config WHERE key = ?1", params![key])
                .ok();
        });
    }

    /// Get the JWT secret (stored in app_config during first run)
    pub fn get_jwt_secret() -> Option<String> {
        Self::get("jwt_secret")
    }

    /// Persist a generated JWT secret
    pub fn set_jwt_secret(secret: &str) {
        Self::set("jwt_secret", secret);
    }
}
