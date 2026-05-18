//! Auth middleware integration tests

use cloudcli_server::auth::middleware;
use std::sync::Once;

static INIT: Once = Once::new();

fn ensure_test_env() {
    INIT.call_once(|| {
        std::env::set_var("JWT_SECRET", "test-jwt-secret-for-auth-tests");
    });
}

// ── Token generation and verification ───────────────────────────────────────

#[test]
fn test_generate_and_verify_token() {
    ensure_test_env();
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
    ensure_test_env();
    let token = middleware::generate_token(1, "testuser");
    let claims = middleware::verify_token_for_test(&token).unwrap();
    let duration = claims.exp - claims.iat;
    // 7 days in seconds = 604800
    assert_eq!(duration, 604800, "Token should expire in exactly 7 days");
}

#[test]
fn test_tampered_token_fails() {
    ensure_test_env();
    let mut token = middleware::generate_token(1, "testuser");
    // Tamper with the token by changing a character in the payload
    token.push('x');
    let claims = middleware::verify_token_for_test(&token);
    assert!(claims.is_none(), "Tampered token should fail verification");
}

#[test]
fn test_expired_token_fails() {
    ensure_test_env();
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
    ensure_test_env();
    let result = middleware::authenticate_websocket(None);
    // In non-platform mode without a token, should return None
    assert!(result.is_none(), "No token should return None");
}

#[test]
fn test_authenticate_websocket_with_invalid_token() {
    ensure_test_env();
    let result = middleware::authenticate_websocket(Some("invalid-token-here"));
    assert!(result.is_none(), "Invalid token should return None");
}
