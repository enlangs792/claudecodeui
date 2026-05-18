//! GitHub Tokens repository — mirrors TS server/modules/database/repositories/github-tokens.ts

use diesel::prelude::*;
use crate::db::connection::with_db;
use crate::db::models::{GitHubToken, NewGitHubToken};
use crate::db::schema::github_tokens;

pub struct GitHubTokensRepo;

impl GitHubTokensRepo {
    /// Store a new GitHub token for a user.
    pub fn create(user_id: i32, token: &str, token_name: Option<&str>) -> Result<GitHubToken, String> {
        let new_token = NewGitHubToken {
            user_id,
            token: token.to_string(),
            token_name: token_name.map(|s| s.to_string()),
        };

        with_db(|conn| {
            diesel::insert_into(github_tokens::table)
                .values(&new_token)
                .returning(GitHubToken::as_returning())
                .get_result(conn)
        })
        .map_err(|e| format!("Failed to create GitHub token: {e}"))
    }

    /// List all GitHub tokens for a user.
    pub fn list_by_user(user_id: i32) -> Result<Vec<GitHubToken>, String> {
        with_db(|conn| {
            github_tokens::table
                .filter(github_tokens::user_id.eq(user_id))
                .order(github_tokens::created_at.desc())
                .load::<GitHubToken>(conn)
        })
        .map_err(|e| format!("Failed to list GitHub tokens: {e}"))
    }

    /// Get a specific GitHub token by ID.
    pub fn get_by_id(token_id: i32) -> Result<Option<GitHubToken>, String> {
        with_db(|conn| {
            github_tokens::table
                .filter(github_tokens::id.eq(token_id))
                .first::<GitHubToken>(conn)
                .optional()
        })
        .map_err(|e| format!("Failed to get GitHub token: {e}"))
    }

    /// Update the last_used timestamp for a token.
    pub fn update_last_used(token_id: i32) -> Result<(), String> {
        with_db(|conn| {
            diesel::update(github_tokens::table.filter(github_tokens::id.eq(token_id)))
                .set(github_tokens::last_used.eq(diesel::dsl::now))
                .execute(conn)
        })
        .map_err(|e| format!("Failed to update GitHub token last_used: {e}"))?;
        Ok(())
    }

    /// Delete a GitHub token by ID.
    pub fn delete(token_id: i32) -> Result<(), String> {
        with_db(|conn| {
            diesel::delete(github_tokens::table.filter(github_tokens::id.eq(token_id)))
                .execute(conn)
        })
        .map_err(|e| format!("Failed to delete GitHub token: {e}"))?;
        Ok(())
    }
}
