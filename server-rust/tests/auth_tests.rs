//! Auth middleware integration tests

use cloudcli_server::auth::middleware;
use cloudcli_server::db::connection;
use cloudcli_server::db::migrations;
use cloudcli_server::db::schema::INIT_SCHEMA_SQL;
use cloudcli_server::db::repos::users::UserRepo;
use cloudcli_server::db::repos::app_config::AppConfigRepo;
use rusqlite::Connection;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(INIT_SCHEMA_SQL).expect("Schema init");
    migrations::run_migrations(&conn);
    conn
}

fn setup_with_user() {
    let _conn = setup_test_db();
    // Insert the test connection into the global singleton
    // Note: The singleton pattern makes this tricky.
    // For now, we test the token functions which don't need the DB connection.
}

// ── Token generation and verification ───────────────────────────────────────

#[test]
fn test_generate_and_verify_token() {
    let token = middleware::generate_token(1, "testuser");
    assert!(!token.is_empty());

    let claims = middleware::verify_token_for_test(&token);
    assert!(claims.is_some(), "Token should verify successfully");
    let c = claims.unwrap();
    assert_eq!(c.username, "testuser");
    assert_eq!(c.userId, 1);
}

#[test]
fn test_token_expiry_is_7_days() {
    let token = middleware::generate_token(1, "testuser");
    let claims = middleware::verify_token_for_test(&token).unwrap();
    let duration = claims.exp - claims.iat;
    // 7 days in seconds = 604800
    assert_eq!(duration, 604800, "Token should expire in exactly 7 days");
}

#[test]
fn test_tampered_token_fails() {
    let mut token = middleware::generate_token(1, "testuser");
    // Tamper with the token by changing a character in the payload
    token.push('x');
    let claims = middleware::verify_token_for_test(&token);
    assert!(claims.is_none(), "Tampered token should fail verification");
}

#[test]
fn test_expired_token_fails() {
    // Create a token that expired 1 day ago
    use jsonwebtoken::{encode, EncodingKey, Header};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;

    let claims = middleware::Claims {
        userId: 1,
        username: "testuser".into(),
        iat: now - 86400 * 2,
        exp: now - 86400,
    };

    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "test-secret-for-expired-test".into());
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    // This should fail because the token is expired
    let result = middleware::verify_token_for_test(&token);
    assert!(result.is_none(), "Expired token should fail verification");
}

// ── WebSocket auth ───────────────────────────────────────────────────────────

#[test]
fn test_authenticate_websocket_without_token() {
    let result = middleware::authenticate_websocket(None);
    // In non-platform mode without a token, should return None
    assert!(result.is_none(), "No token should return None");
}

#[test]
fn test_authenticate_websocket_with_invalid_token() {
    let result = middleware::authenticate_websocket(Some("invalid-token-here"));
    assert!(result.is_none(), "Invalid token should return None");
}
