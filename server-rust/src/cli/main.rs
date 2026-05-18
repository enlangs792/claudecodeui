//! CloudCLI CLI — mirrors server/cli.js
//!
//! Commands:
//!   (no args) / start   Start the server (default)
//!   sandbox             Manage Docker sandbox environments
//!   status / info       Show configuration and data locations
//!   update              Update to the latest version
//!   help                Show help information
//!   version             Show version information

mod color;
mod sandbox;

use std::path::PathBuf;

// ── App root resolution ─────────────────────────────────────────────────────

fn find_app_root() -> PathBuf {
    // Try CARGO_MANIFEST_DIR first (works in dev)
    if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(&dir);
        // CARGO_MANIFEST_DIR points to server-rust/, app root is parent
        if p.join("server-rust").exists() {
            return p;
        }
        if p.join("dist").exists() {
            return p;
        }
    }
    // Walk up from current exe
    if let Ok(exe) = std::env::current_exe() {
        let mut current = exe.parent().map(|p| p.to_path_buf());
        while let Some(dir) = current {
            if dir.join("dist").exists() && dir.join("server-rust").exists() {
                return dir;
            }
            if let Some(parent) = dir.parent() {
                current = Some(parent.to_path_buf());
            } else {
                break;
            }
        }
    }
    // Fallback to CWD
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn default_database_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".cloudcli")
        .join("database.sqlite")
}

// ── Version ─────────────────────────────────────────────────────────────────

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Semantic version comparison ─────────────────────────────────────────────

fn is_newer_version(v1: &str, v2: &str) -> bool {
    let parts1: Vec<u32> = v1.split('.').filter_map(|p| p.parse().ok()).collect();
    let parts2: Vec<u32> = v2.split('.').filter_map(|p| p.parse().ok()).collect();
    for i in 0..3 {
        let a = parts1.get(i).copied().unwrap_or(0);
        let b = parts2.get(i).copied().unwrap_or(0);
        if a > b { return true; }
        if a < b { return false; }
    }
    false
}

// ── Update check ────────────────────────────────────────────────────────────

fn check_for_updates(silent: bool) {
    use color::*;
    let current_version = VERSION;

    match std::process::Command::new("npm")
        .args(["show", "@cloudcli-ai/cloudcli", "version"])
        .output()
    {
        Ok(output) => {
            let latest_version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if is_newer_version(&latest_version, current_version) {
                println!(
                    "\n{} New version available: {} (current: {})",
                    warn("[UPDATE]"),
                    bright(&latest_version),
                    current_version
                );
                println!(
                    "         Run {} to update\n",
                    bright("cloudcli update")
                );
            } else if !silent {
                println!(
                    "{} You are on the latest version ({})",
                    ok("[OK]"),
                    current_version
                );
            }
        }
        Err(_) => {
            if !silent {
                println!("{} Could not check for updates", warn("[WARN]"));
            }
        }
    }
}

fn update_package() {
    use color::*;
    println!("{} Checking for updates...", info("[INFO]"));

    let current_version = VERSION;
    match std::process::Command::new("npm")
        .args(["show", "@cloudcli-ai/cloudcli", "version"])
        .output()
    {
        Ok(output) => {
            let latest_version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !is_newer_version(&latest_version, current_version) {
                println!(
                    "{} Already on the latest version ({})",
                    ok("[OK]"),
                    current_version
                );
                return;
            }
            println!(
                "{} Updating from {} to {}...",
                info("[INFO]"),
                current_version,
                latest_version
            );
            match std::process::Command::new("npm")
                .args(["update", "-g", "@cloudcli-ai/cloudcli"])
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
            {
                Ok(status) if status.success() => {
                    println!(
                        "{} Update complete! Restart cloudcli to use the new version.",
                        ok("[OK]")
                    );
                }
                _ => {
                    eprintln!("{} Update failed", error("[ERROR]"));
                    println!(
                        "{} Try running manually: npm update -g @cloudcli-ai/cloudcli",
                        tip("[TIP]")
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("{} Update failed: {e}", error("[ERROR]"));
            println!(
                "{} Try running manually: npm update -g @cloudcli-ai/cloudcli",
                tip("[TIP]")
            );
        }
    }
}

// ── Commands ────────────────────────────────────────────────────────────────

const HELP: &str = r#"
╔═══════════════════════════════════════════════════════════════╗
║              CloudCLI - Command Line Tool (Rust)              ║
╚═══════════════════════════════════════════════════════════════╝

Usage:
  cloudcli [command] [options]

Commands:
  start          Start the CloudCLI server (default)
  sandbox        Manage Docker sandbox environments
  status         Show configuration and data locations
  update         Update to the latest version
  help           Show this help information
  version        Show version information

Options:
  -p, --port <port>           Set server port (default: 3001)
  --database-path <path>      Set custom database location
  -h, --help                  Show this help information
  -v, --version               Show version information

Examples:
  $ cloudcli                        # Start with defaults
  $ cloudcli --port 8080            # Start on port 8080
  $ cloudcli sandbox ~/my-project   # Run in a Docker sandbox
  $ cloudcli status                 # Show configuration

Environment Variables:
  SERVER_PORT         Set server port (default: 3001)
  PORT                Set server port (default: 3001) (LEGACY)
  DATABASE_PATH       Set custom database location
  CLAUDE_CLI_PATH     Set custom Claude CLI path
  CONTEXT_WINDOW      Set context window size (default: 160000)

Documentation:
  https://cloudcli.ai
"#;

fn show_version() {
    println!("{VERSION}");
}

fn show_help() {
    println!("{HELP}");
}

fn show_status() {
    use color::*;

    let app_root = find_app_root();
    let db_path = std::env::var("DATABASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_database_path());
    let server_port = std::env::var("SERVER_PORT")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| "3001".into());
    let claude_cli_path = std::env::var("CLAUDE_CLI_PATH").unwrap_or_else(|_| "claude (default)".into());
    let context_window = std::env::var("CONTEXT_WINDOW").unwrap_or_else(|_| "160000 (default)".into());

    println!("\n{}", bright("CloudCLI UI - Status\n"));
    println!("{}", dim("═".repeat(60).as_str()));

    println!("\n{} Version: {}", info("[INFO]"), bright(VERSION));

    println!("\n{} Installation Directory:", info("[INFO]"));
    println!("       {}", dim(&app_root.display().to_string()));

    println!("\n{} Database Location:", info("[INFO]"));
    println!("       {}", dim(&db_path.display().to_string()));
    if db_path.exists() {
        if let Ok(meta) = std::fs::metadata(&db_path) {
            println!("       Status: {}", ok("[OK] Exists"));
            println!("       Size: {}", dim(&format!("{:.2} KB", meta.len() as f64 / 1024.0)));
            if let Ok(modified) = meta.modified() {
                println!(
                    "       Modified: {}",
                    dim(&format!(
                        "{}",
                        chrono::DateTime::<chrono::Utc>::from(modified).format("%Y-%m-%d %H:%M:%S")
                    ))
                );
            }
        }
    } else {
        println!("       Status: {}", warn("[WARN] Not created yet (will be created on first run)"));
    }

    println!("\n{} Configuration:", info("[INFO]"));
    println!("       SERVER_PORT: {} {}", bright(&server_port), dim(if std::env::var("SERVER_PORT").is_ok() || std::env::var("PORT").is_ok() { "" } else { "(default)" }));
    println!(
        "       DATABASE_PATH: {}",
        dim(&std::env::var("DATABASE_PATH").unwrap_or_else(|_| "(using default location)".into()))
    );
    println!("       CLAUDE_CLI_PATH: {}", dim(&claude_cli_path));
    println!("       CONTEXT_WINDOW: {}", dim(&context_window));

    // Claude projects folder
    let claude_projects = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claude")
        .join("projects");
    println!("\n{} Claude Projects Folder:", info("[INFO]"));
    println!("       {}", dim(&claude_projects.display().to_string()));
    if claude_projects.exists() {
        println!("       Status: {}", ok("[OK] Exists"));
    } else {
        println!("       Status: {}", warn("[WARN] Not found"));
    }

    // .env file
    let env_file = app_root.join(".env");
    println!("\n{} Configuration File:", info("[INFO]"));
    println!("       {}", dim(&env_file.display().to_string()));
    if env_file.exists() {
        println!("       Status: {}", ok("[OK] Exists"));
    } else {
        println!("       Status: {}", warn("[WARN] Not found (using defaults)"));
    }

    println!("\n{}", dim("═".repeat(60).as_str()));
    println!("\n{} Hints:", tip("[TIP]"));
    println!("      {} Use {} to run on a custom port", dim(">"), bright("cloudcli --port 8080"));
    println!(
        "      {} Use {} for custom database",
        dim(">"),
        bright("cloudcli --database-path /path/to/db")
    );
    println!("      {} Run {} for all options", dim(">"), bright("cloudcli help"));
    println!(
        "      {} Access the UI at http://localhost:{}",
        dim(">"),
        server_port
    );
    println!();
}

// ── Argument parsing ────────────────────────────────────────────────────────

struct CliArgs {
    command: String,
    server_port: Option<String>,
    database_path: Option<String>,
    remaining_args: Vec<String>,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut command = String::from("start");
    let mut server_port = None;
    let mut database_path = None;
    let mut remaining_args = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                i += 1;
                if i < args.len() {
                    server_port = Some(args[i].clone());
                }
            }
            "--database-path" => {
                i += 1;
                if i < args.len() {
                    database_path = Some(args[i].clone());
                }
            }
            "--help" | "-h" => command = "help".into(),
            "--version" | "-v" => command = "version".into(),
            a if a.starts_with("--port=") => {
                server_port = Some(a.split('=').nth(1).unwrap_or("3001").into());
            }
            a if a.starts_with("--database-path=") => {
                database_path = Some(a.split('=').nth(1).unwrap_or("").into());
            }
            a if !a.starts_with('-') => {
                command = a.into();
                if command == "sandbox" {
                    remaining_args = args[(i + 1)..].to_vec();
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }

    CliArgs {
        command,
        server_port,
        database_path,
        remaining_args,
    }
}

// ── Environment file loading ────────────────────────────────────────────────

fn load_env_file() {
    let env_path = find_app_root().join(".env");
    if let Ok(content) = std::fs::read_to_string(&env_path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                if std::env::var(key).is_err() {
                    std::env::set_var(key, value);
                }
            }
        }
    }
}

// ── Server start ────────────────────────────────────────────────────────────

fn start_server() -> anyhow::Result<()> {
    // Auto-check for updates silently on startup (mirrors TS behavior)
    check_for_updates(true);

    // This spawns the actual Axum server in the current process.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        use cloudcli_server::db::connection::init_pool;
        use cloudcli_server::db::migrations::initialize_database;

        init_pool();
        initialize_database();

        let app_root = find_app_root();
        tracing::info!("App root resolved to: {}", app_root.display());

        let cors = tower_http::cors::CorsLayer::permissive().expose_headers([
            axum::http::header::HeaderName::from_static("x-refreshed-token"),
        ]);

        // Protected API middleware stack
        let mut protected = axum::Router::new()
            .nest("/projects", cloudcli_server::routes::projects::routes())
            .nest("/git", cloudcli_server::routes::git::routes())
            .nest("/user", cloudcli_server::routes::user::routes())
            .nest("/settings", cloudcli_server::routes::settings::routes())
            .nest("/commands", cloudcli_server::routes::commands::routes())
            .nest("/agent", cloudcli_server::routes::agent::routes())
            .nest("/taskmaster", cloudcli_server::routes::taskmaster::routes())
            .nest("/mcp-utils", cloudcli_server::routes::mcp_utils::routes())
            .nest("/cursor", cloudcli_server::routes::cursor::routes())
            .nest("/gemini", cloudcli_server::routes::gemini::routes())
            .nest("/plugins", cloudcli_server::routes::plugins::routes())
            .merge(cloudcli_server::routes::filesystem::routes());
        protected = protected.layer(axum::middleware::from_fn(
            cloudcli_server::auth::middleware::authenticate_token,
        ));

        let api_routes = axum::Router::new()
            .nest("/auth", cloudcli_server::routes::auth::routes())
            .merge(protected)
            .layer(axum::middleware::from_fn(
                cloudcli_server::auth::middleware::validate_api_key,
            ));

        let dist_path = app_root.join("dist");
        let public_path = app_root.join("public");

        let app = axum::Router::new()
            .route("/health", axum::routing::get(|| async {
                axum::response::Json(serde_json::json!({
                    "status": "ok",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "installMode": "git",
                    "server": "rust"
                }))
            }))
            .nest("/api", api_routes)
            .merge(cloudcli_server::ws::server::ws_router())
            .layer(cors);

        // Static file serving
        let app = if dist_path.exists() {
            app.fallback_service(tower_http::services::ServeDir::new(&dist_path))
        } else {
            let public = public_path.clone();
            app.fallback_service(tower::service_fn(move |_req: axum::http::Request<axum::body::Body>| {
                let public = public.clone();
                async move {
                    let path = _req.uri().path().trim_start_matches('/');
                    if !path.is_empty() {
                        let file = public.join(path);
                        if file.is_file() {
                            if let Ok(data) = tokio::fs::read(&file).await {
                                let ct = mime_guess::from_path(&file).first_or_octet_stream().to_string();
                                let resp = axum::http::Response::builder()
                                    .header("Content-Type", ct)
                                    .body(axum::body::Body::from(data))
                                    .unwrap();
                                return Ok::<_, std::convert::Infallible>(resp);
                            }
                        }
                    }
                    let redirect = format!("http://localhost:{}", 5173);
                    let resp = axum::http::Response::builder()
                        .status(axum::http::StatusCode::FOUND)
                        .header("Location", redirect)
                        .body(axum::body::Body::empty())
                        .unwrap();
                    Ok(resp)
                }
            }))
        };

        let port: u16 = std::env::var("SERVER_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3001);
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let addr = format!("{host}:{port}");
        tracing::info!("CloudCLI Rust Server starting on {addr}");

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;
        Ok::<_, anyhow::Error>(())
    })
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args = parse_args();

    // Apply CLI options
    if let Some(port) = args.server_port {
        std::env::set_var("SERVER_PORT", &port);
    }
    if let Some(db_path) = args.database_path {
        std::env::set_var("DATABASE_PATH", &db_path);
    }
    // Legacy PORT env fallback
    if std::env::var("SERVER_PORT").is_err() {
        if let Ok(port) = std::env::var("PORT") {
            std::env::set_var("SERVER_PORT", port);
        }
    }

    load_env_file();

    match args.command.as_str() {
        "start" => {
            if let Err(e) = start_server() {
                eprintln!("\n❌ Error: {e}");
                std::process::exit(1);
            }
        }
        "sandbox" => {
            if let Err(e) = sandbox::sandbox_command_sync(&args.remaining_args) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        "status" | "info" => show_status(),
        "help" => show_help(),
        "version" => show_version(),
        "update" => update_package(),
        other => {
            eprintln!("\n❌ Unknown command: {other}");
            eprintln!("   Run \"cloudcli help\" for usage information.\n");
            std::process::exit(1);
        }
    }
}
