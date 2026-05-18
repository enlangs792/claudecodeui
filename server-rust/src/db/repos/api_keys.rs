//! API Keys repository — Diesel implementation

use diesel::prelude::*;
use crate::db::connection;
use crate::db::models;
use crate::db::schema::api_keys;

#[derive(Debug, Clone)]
pub struct ApiKeyRow {
    pub id: i64,
    pub user_id: i64,
    pub key_name: String,
    pub api_key: String,
    pub created_at: Option<chrono::NaiveDateTime>,
    pub last_used: Option<chrono::NaiveDateTime>,
    pub is_active: bool,
}

impl From<models::ApiKey> for ApiKeyRow {
    fn from(k: models::ApiKey) -> Self {
        ApiKeyRow {
            id: k.id,
            user_id: k.user_id,
            key_name: k.key_name,
            api_key: k.api_key,
            created_at: k.created_at,
            last_used: k.last_used,
            is_active: k.is_active,
        }
    }
}

pub struct ApiKeysRepo;

impl ApiKeysRepo {
    pub fn create(user_id: i64, key_name: &str, api_key: &str) -> i64 {
        connection::with_db(|conn| {
            use api_keys::dsl;

            let new_key = models::NewApiKey {
                user_id,
                key_name: key_name.to_string(),
                api_key: api_key.to_string(),
            };

            diesel::insert_into(api_keys::table)
                .values(&new_key)
                .returning(api_keys::id)
                .get_result::<i64>(conn)
                .expect("Failed to create API key")
        })
    }

    pub fn validate(api_key_str: &str) -> Option<i64> {
        connection::with_db(|conn| {
            use api_keys::dsl;

            let result: Option<i64> = dsl::api_keys
                .filter(dsl::api_key.eq(api_key_str))
                .filter(dsl::is_active.eq(true))
                .select(dsl::user_id)
                .first(conn)
                .ok();

            if result.is_some() {
                let now = chrono::Utc::now().naive_utc();
                diesel::update(
                    dsl::api_keys.filter(dsl::api_key.eq(api_key_str)),
                )
                .set(dsl::last_used.eq(Some(now)))
                .execute(conn)
                .ok();
            }

            result
        })
    }

    pub fn list_by_user(user_id: i64) -> Vec<ApiKeyRow> {
        connection::with_db(|conn| {
            use api_keys::dsl;
            dsl::api_keys
                .filter(dsl::user_id.eq(user_id))
                .filter(dsl::is_active.eq(true))
                .order(dsl::created_at.desc())
                .load::<models::ApiKey>(conn)
                .unwrap_or_default()
                .into_iter()
                .map(ApiKeyRow::from)
                .collect()
        })
    }

    pub fn deactivate(key_id: i64) {
        connection::with_db(|conn| {
            use api_keys::dsl;
            diesel::update(dsl::api_keys.filter(dsl::id.eq(key_id)))
                .set(dsl::is_active.eq(false))
                .execute(conn)
                .ok();
        });
    }
}
