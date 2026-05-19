//! CloudCLI Server library — Rust rewrite
//!
//! Public API surface for integration tests and binary crates.
pub mod db;
pub mod auth;
pub mod routes;
pub mod ws;
pub mod providers;
pub mod services;
pub mod shared;
#[cfg(feature = "acp-bridge")]
pub mod acp;
