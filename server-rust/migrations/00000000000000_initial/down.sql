-- Rollback: drop all tables in reverse dependency order

DROP TABLE IF EXISTS push_subscriptions;
DROP TABLE IF EXISTS vapid_keys;
DROP TABLE IF EXISTS user_notification_preferences;
DROP TABLE IF EXISTS user_credentials;
DROP TABLE IF EXISTS api_keys;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS scan_state;
DROP TABLE IF EXISTS github_tokens;
DROP TABLE IF EXISTS app_config;
DROP TABLE IF EXISTS projects;
DROP TABLE IF EXISTS users;
