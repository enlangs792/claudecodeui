//! Credentials repository — Diesel implementation

use diesel::prelude::*;
use crate::db::connection;
use crate::db::models;
use crate::db::schema::user_credentials;
use crate::shared::types::{CreateCredentialResult, CredentialPublicRow};

pub struct CredentialsRepo;

impl CredentialsRepo {
    pub fn create(
        user_id: i64,
        name: &str,
        cred_type: &str,
        value: &str,
        description: Option<&str>,
    ) -> CreateCredentialResult {
        connection::with_db(|conn| {
            use user_credentials::dsl;

            let new_cred = models::NewCredential {
                user_id,
                credential_name: name.to_string(),
                credential_type: cred_type.to_string(),
                credential_value: value.to_string(),
                description: description.map(String::from),
            };

            let id: i64 = diesel::insert_into(user_credentials::table)
                .values(&new_cred)
                .returning(user_credentials::id)
                .get_result(conn)
                .expect("Failed to create credential");

            CreateCredentialResult {
                id,
                credential_name: name.to_string(),
                credential_type: cred_type.to_string(),
            }
        })
    }

    pub fn list_by_user(user_id: i64) -> Vec<CredentialPublicRow> {
        connection::with_db(|conn| {
            use user_credentials::dsl;
            dsl::user_credentials
                .filter(dsl::user_id.eq(user_id))
                .filter(dsl::is_active.eq(true))
                .order(dsl::created_at.desc())
                .select((
                    dsl::id,
                    dsl::credential_name,
                    dsl::credential_type,
                    dsl::description,
                    dsl::created_at,
                    dsl::is_active,
                ))
                .load::<(i64, String, String, Option<String>, Option<chrono::NaiveDateTime>, bool)>(conn)
                .unwrap_or_default()
                .into_iter()
                .map(|(id, credential_name, credential_type, description, created_at, is_active)| {
                    CredentialPublicRow {
                        id,
                        credential_name,
                        credential_type,
                        description,
                        created_at: created_at
                            .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string())
                            .unwrap_or_default(),
                        is_active: is_active as i32,
                    }
                })
                .collect()
        })
    }

    pub fn deactivate(credential_id: i64) {
        connection::with_db(|conn| {
            use user_credentials::dsl;
            diesel::update(
                dsl::user_credentials.filter(dsl::id.eq(credential_id)),
            )
            .set(dsl::is_active.eq(false))
            .execute(conn)
            .ok();
        });
    }

    pub fn get_value(user_id: i64, credential_name: &str) -> Option<String> {
        connection::with_db(|conn| {
            use user_credentials::dsl;
            dsl::user_credentials
                .filter(dsl::user_id.eq(user_id))
                .filter(dsl::credential_name.eq(credential_name))
                .filter(dsl::is_active.eq(true))
                .select(dsl::credential_value)
                .first::<String>(conn)
                .ok()
        })
    }
}
