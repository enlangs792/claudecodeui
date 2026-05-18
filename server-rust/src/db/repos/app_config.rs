//! App Config repository — Diesel implementation

use diesel::prelude::*;
use crate::db::connection;
use crate::db::models;
use crate::db::schema::app_config;

pub struct AppConfigRepo;

impl AppConfigRepo {
    pub fn get(key: &str) -> Option<String> {
        connection::with_db(|conn| {
            use app_config::dsl;
            dsl::app_config
                .filter(dsl::key.eq(key))
                .select(dsl::value)
                .first::<String>(conn)
                .ok()
        })
    }

    pub fn set(key: &str, value: &str) {
        connection::with_db(|conn| {
            use app_config::dsl;

            let new_config = models::NewAppConfig {
                key: key.to_string(),
                value: value.to_string(),
            };

            diesel::insert_into(app_config::table)
                .values(&new_config)
                .on_conflict(dsl::key)
                .do_update()
                .set(dsl::value.eq(value.to_string()))
                .execute(conn)
                .ok();
        });
    }

    pub fn delete(key: &str) {
        connection::with_db(|conn| {
            use app_config::dsl;
            diesel::delete(dsl::app_config.filter(dsl::key.eq(key)))
                .execute(conn)
                .ok();
        });
    }

    pub fn get_jwt_secret() -> Option<String> {
        Self::get("jwt_secret")
    }

    pub fn set_jwt_secret(secret: &str) {
        Self::set("jwt_secret", secret);
    }
}
