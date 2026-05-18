mod db;
mod auth;
mod routes;
mod ws;
mod providers;
mod services;
mod shared;

use axum::{middleware, Router, response::Json, routing::get};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;
use tracing_subscriber::{EnvFilter, fmt};

use crate::db::migrations::initialize_database;
use crate::db::connection::get_connection;
use crate::auth::middleware as auth_mw;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // Initialize database schema and run migrations
    {
        let guard = get_connection();
        let conn = guard.as_ref().expect("Database not initialized");
        initialize_database(conn);
    }

    let cors = CorsLayer::permissive()
        .expose_headers([axum::http::header::HeaderName::from_static("x-refreshed-token")]);

    // Protected API middleware stack
    let protected = Router::new()
        .nest("/projects", routes::projects::routes());

    let api_routes = Router::new()
        // Auth routes — public (no auth required)
        .nest("/auth", routes::auth::routes())
        // Protected routes
        .nest("/", protected.layer(middleware::from_fn(auth_mw::authenticate_token)))
        .layer(middleware::from_fn(auth_mw::validate_api_key));

    let app = Router::new()
        .route("/health", get(health_check))
        .nest("/api", api_routes)
        .merge(ws::server::ws_router())
        .layer(cors);

    let port: u16 = std::env::var("SERVER_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());

    let addr = format!("{}:{}", host, port);
    tracing::info!("CloudCLI Rust Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "installMode": "git",
        "server": "rust"
    }))
}
