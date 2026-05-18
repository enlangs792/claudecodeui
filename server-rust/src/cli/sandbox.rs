//! Sandbox command — mirrors server/cli.js sandboxCommand()
//!
//! Manages Docker sandbox environments via the `sbx` CLI.
//! Subcommands: create (default), ls, stop, start, rm, logs, help

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::color;

const SANDBOX_TEMPLATES: &[(&str, &str)] = &[
    ("claude", "docker.io/cloudcliai/sandbox:claude-code"),
    ("codex", "docker.io/cloudcliai/sandbox:codex"),
    ("gemini", "docker.io/cloudcliai/sandbox:gemini"),
];

const SANDBOX_SECRETS: &[(&str, &str)] = &[
    ("claude", "anthropic"),
    ("codex", "openai"),
    ("gemini", "google"),
];

fn template_for(agent: &str) -> &str {
    SANDBOX_TEMPLATES
        .iter()
        .find(|(a, _)| *a == agent)
        .map(|(_, t)| *t)
        .unwrap_or("docker.io/cloudcliai/sandbox:claude-code")
}

fn secret_for(agent: &str) -> &str {
    SANDBOX_SECRETS
        .iter()
        .find(|(a, _)| *a == agent)
        .map(|(_, s)| *s)
        .unwrap_or("anthropic")
}

pub struct SandboxOpts {
    pub subcommand: String,
    pub workspace: Option<String>,
    pub agent: String,
    pub name: Option<String>,
    pub port: u16,
    pub template: Option<String>,
    pub env: Vec<String>,
}

pub fn parse_sandbox_args(args: &[String]) -> SandboxOpts {
    let subcommands = ["ls", "stop", "start", "rm", "logs", "help"];
    let mut subcommand = None;
    let mut workspace = None;
    let mut agent = "claude".to_string();
    let mut name = None;
    let mut port: u16 = 3001;
    let mut template = None;
    let mut env_vars = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            a if i == 0 && subcommands.contains(&a) => {
                subcommand = Some(a.to_string());
            }
            "--agent" | "-a" => {
                i += 1;
                if i < args.len() {
                    agent = args[i].clone();
                }
            }
            "--name" | "-n" => {
                i += 1;
                if i < args.len() {
                    name = Some(args[i].clone());
                }
            }
            "--port" => {
                i += 1;
                if i < args.len() {
                    port = args[i].parse().unwrap_or(3001);
                }
            }
            "--template" | "-t" => {
                i += 1;
                if i < args.len() {
                    template = Some(args[i].clone());
                }
            }
            "--env" | "-e" => {
                i += 1;
                if i < args.len() {
                    env_vars.push(args[i].clone());
                }
            }
            a if !a.starts_with('-') => {
                if subcommand.is_none() {
                    workspace = Some(a.to_string());
                } else {
                    name = Some(a.to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }

    let subcommand = subcommand.unwrap_or_else(|| "create".to_string());

    // Derive name from workspace path if not set
    if name.is_none() {
        if let Some(ref ws) = workspace {
            let resolved = if ws.starts_with('~') {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
                PathBuf::from(ws.replace('~', &home.display().to_string()))
            } else {
                PathBuf::from(ws)
            };
            name = resolved
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_string());
        }
    }

    // Default template from agent
    if template.is_none() {
        template = Some(template_for(&agent).to_string());
    }

    SandboxOpts {
        subcommand,
        workspace,
        agent,
        name,
        port,
        template,
        env: env_vars,
    }
}

fn sbx(args: &[&str], inherit: bool) -> Result<String, String> {
    let output = Command::new("sbx")
        .args(args)
        .stdout(if inherit {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .stderr(if inherit {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .output()
        .map_err(|e| format!("Failed to run sbx: {e}"))?;

    if inherit {
        Ok(String::new())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            let msg = if !stderr.is_empty() { stderr } else { stdout };
            return Err(msg.trim().to_string());
        }
        Ok(stdout)
    }
}

fn sbx_spawn_detached(args: &[&str]) {
    let _ = Command::new("sbx")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn check_sbx_installed() -> Result<(), String> {
    sbx(&["version"], false).map_err(|_| {
        format!(
            "\n{} sbx CLI not found.\n\n\
             Install it from: {}\n\
             Then run: {}\n\
             And store your API key: {}\n",
            color::error("❌"),
            color::info("https://docs.docker.com/ai/sandboxes/get-started/"),
            color::bright("sbx login"),
            color::bright("sbx secret set -g anthropic"),
        )
    })?;
    Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        Ok(())
    } else {
        Err(format!(
            "\n{} Invalid sandbox name: {}\n   Names may only contain letters, numbers, hyphens, and underscores.\n",
            color::error("❌"),
            name
        ))
    }
}

pub fn show_sandbox_help() {
    println!(
        r#"
{} — Run CloudCLI inside Docker Sandboxes

Usage:
  cloudcli sandbox <workspace>            Create and start a sandbox
  cloudcli sandbox <subcommand> [name]    Manage sandboxes

Subcommands:
  {}    Create a sandbox and start the web UI
  {}           List all sandboxes
  {}        Restart a stopped sandbox and re-launch the web UI
  {}         Stop a sandbox (preserves state)
  {}           Remove a sandbox
  {}         Show CloudCLI server logs
  {}         Show this help

Options:
  -a, --agent <agent>       Agent to use: claude, codex, gemini (default: claude)
  -n, --name <name>         Sandbox name (default: derived from workspace folder)
  -t, --template <image>    Custom template image
  -e, --env <KEY=VALUE>     Set environment variable (repeatable)
      --port <port>         Host port for the web UI (default: 3001)

Examples:
  $ cloudcli sandbox ~/my-project
  $ cloudcli sandbox ~/my-project --agent codex --port 8080
  $ cloudcli sandbox ~/my-project --env SERVER_PORT=8080 --env HOST=0.0.0.0
  $ cloudcli sandbox ls
  $ cloudcli sandbox stop my-project
  $ cloudcli sandbox start my-project
  $ cloudcli sandbox rm my-project

Prerequisites:
  1. Install sbx CLI: https://docs.docker.com/ai/sandboxes/get-started/
  2. Authenticate and store your API key:
       sbx login
       sbx secret set -g anthropic   # for Claude
       sbx secret set -g openai      # for Codex
       sbx secret set -g google      # for Gemini

Advanced usage:
  For branch mode, multiple workspaces, memory limits, network policies,
  or passing prompts to the agent, use sbx directly with the template:

    sbx run --template docker.io/cloudcliai/sandbox:claude-code claude ~/my-project --branch my-feature
    sbx run --template docker.io/cloudcliai/sandbox:claude-code claude ~/project ~/libs:ro --memory 8g

  Full Docker Sandboxes docs: https://docs.docker.com/ai/sandboxes/usage/
"#,
        color::bright("CloudCLI Sandbox"),
        color::bright("(default)"),
        color::bright("ls"),
        color::bright("start"),
        color::bright("stop"),
        color::bright("rm"),
        color::bright("logs"),
        color::bright("help"),
    );
}

pub fn sandbox_command_sync(args: &[String]) -> Result<(), String> {
    let opts = parse_sandbox_args(args);

    if opts.subcommand == "help" {
        show_sandbox_help();
        return Ok(());
    }

    // Validate name
    if let Some(ref name) = opts.name {
        validate_name(name)?;
    }

    // Check sbx is installed
    check_sbx_installed()?;

    match opts.subcommand.as_str() {
        "ls" => {
            sbx(&["ls"], true)?;
        }

        "stop" => {
            let name = opts.name.as_ref().ok_or_else(|| {
                format!(
                    "\n{} Sandbox name required: cloudcli sandbox stop <name>\n",
                    color::error("❌")
                )
            })?;
            sbx(&["stop", name], true)?;
        }

        "rm" => {
            let name = opts.name.as_ref().ok_or_else(|| {
                format!(
                    "\n{} Sandbox name required: cloudcli sandbox rm <name>\n",
                    color::error("❌")
                )
            })?;
            sbx(&["rm", name], true)?;
        }

        "logs" => {
            let name = opts.name.as_ref().ok_or_else(|| {
                format!(
                    "\n{} Sandbox name required: cloudcli sandbox logs <name>\n",
                    color::error("❌")
                )
            })?;
            sbx(&["exec", name, "bash", "-c", "cat /tmp/cloudcli-ui.log"], true)
                .map_err(|e| format!("\n{} Could not read logs: {}\n", color::error("❌"), e))?;
        }

        "start" => {
            let name = opts.name.as_ref().ok_or_else(|| {
                format!(
                    "\n{} Sandbox name required: cloudcli sandbox start <name>\n",
                    color::error("❌")
                )
            })?;
            println!(
                "\n{} Starting sandbox {}...",
                color::info("▶"),
                color::bright(name)
            );

            // Spawn sbx run in background to restart
            sbx_spawn_detached(&["run", name]);

            // Wait for sandbox to be ready
            std::thread::sleep(std::time::Duration::from_secs(5));

            println!(
                "{} Launching CloudCLI web server...",
                color::info("▶")
            );
            let _ = sbx(
                &["exec", name, "bash", "-c", "cloudcli start --port 3001 &"],
                false,
            );

            let port = opts.port.to_string();
            let port_map = format!("{port}:3001");
            println!(
                "{} Forwarding port {} → 3001...",
                color::info("▶"),
                port
            );

            match sbx(&["ports", name, "--publish", &port_map], false) {
                Ok(_) => {}
                Err(e) if e.contains("address already in use") => {
                    let alt_port = opts.port + 1;
                    println!(
                        "{} Port {port} in use, trying {alt_port}...",
                        color::warn("⚠")
                    );
                    let alt_map = format!("{alt_port}:3001");
                    sbx(&["ports", name, "--publish", &alt_map], false).map_err(|e2| {
                        format!(
                            "{} Ports {port} and {alt_port} both in use. Use --port to specify a free port.\n{e2}",
                            color::error("❌")
                        )
                    })?;
                }
                Err(e) => return Err(e),
            }

            println!(
                "\n{} {}",
                color::ok("✔"),
                color::bright("CloudCLI is ready!")
            );
            println!(
                "  {} {}",
                color::info("→"),
                color::bright(&format!("http://localhost:{port}"))
            );
        }

        "create" => {
            let workspace = opts.workspace.as_ref().ok_or_else(|| {
                format!(
                    "\n{} Workspace path required: cloudcli sandbox <path>\n   Example: {}\n",
                    color::error("❌"),
                    color::bright("cloudcli sandbox ~/my-project")
                )
            })?;

            let resolved = if workspace.starts_with('~') {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
                PathBuf::from(workspace.replace('~', &home.display().to_string()))
            } else {
                PathBuf::from(workspace)
            };

            if !resolved.exists() {
                return Err(format!(
                    "\n{} Workspace path not found: {}\n",
                    color::error("❌"),
                    color::dim(&resolved.display().to_string())
                ));
            }

            let secret = secret_for(&opts.agent);
            let template = opts.template.as_deref().unwrap_or("docker.io/cloudcliai/sandbox:claude-code");
            let name = opts.name.as_deref().unwrap_or("unknown");

            // Check if required secret is stored
            if let Ok(secret_list) = sbx(&["secret", "ls"], false) {
                if !secret_list.contains(secret) {
                    eprintln!(
                        "\n{} No {} API key found.\n   Run: {}\n",
                        color::error("❌"),
                        color::bright(secret),
                        color::bright(&format!("sbx secret set -g {secret}"))
                    );
                    std::process::exit(1);
                }
            }

            let workspace_display = resolved.display().to_string();
            println!("\n{}", color::bright("CloudCLI Sandbox"));
            println!("{}", color::dim("─".repeat(50).as_str()));
            println!("  Agent:     {} {}", color::info(&opts.agent), color::dim(&format!("({secret} credentials)")));
            println!("  Workspace: {}", color::dim(&workspace_display));
            println!("  Name:      {}", color::dim(name));
            println!("  Template:  {}", color::dim(template));
            println!("  Port:      {}", color::dim(&opts.port.to_string()));
            if !opts.env.is_empty() {
                println!("  Env:       {}", color::dim(&opts.env.join(", ")));
            }
            println!("{}", color::dim("─".repeat(50).as_str()));

            // Step 1: Launch sandbox in background
            println!(
                "\n{} Creating sandbox {}...",
                color::info("▶"),
                color::bright(name)
            );
            sbx_spawn_detached(&[
                "run",
                "--template",
                template,
                "--name",
                name,
                &opts.agent,
                &workspace_display,
            ]);
            std::thread::sleep(std::time::Duration::from_secs(5));

            // Step 2: Inject environment variables
            if !opts.env.is_empty() {
                println!("{} Setting environment variables...", color::info("▶"));
                let exports: Vec<String> = opts
                    .env
                    .iter()
                    .filter(|e| {
                        if let Some((k, v)) = e.split_once('=') {
                            !k.is_empty() && !v.is_empty()
                        } else {
                            false
                        }
                    })
                    .map(|e| format!("export {e}"))
                    .collect();

                if !exports.is_empty() {
                    let joined = exports.join("\n");
                    let _ = sbx(
                        &[
                            "exec",
                            name,
                            "bash",
                            "-c",
                            &format!("echo '{joined}' >> /etc/sandbox-persistent.sh"),
                        ],
                        false,
                    );
                }

                let invalid: Vec<&String> = opts
                    .env
                    .iter()
                    .filter(|e| {
                        if let Some((k, v)) = e.split_once('=') {
                            k.is_empty() || v.is_empty()
                        } else {
                            true
                        }
                    })
                    .collect();
                if !invalid.is_empty() {
                    let names: Vec<&str> = invalid.iter().map(|s| s.as_str()).collect();
                    println!(
                        "{} Skipped invalid env vars: {} (expected KEY=VALUE)",
                        color::warn("⚠"),
                        names.join(", ")
                    );
                }
            }

            // Step 3: Start CloudCLI inside the sandbox
            println!(
                "{} Launching CloudCLI web server...",
                color::info("▶")
            );
            let _ = sbx(
                &["exec", name, "bash", "-c", "cloudcli start --port 3001 &"],
                false,
            );

            // Step 4: Forward port
            let port_str = opts.port.to_string();
            println!(
                "{} Forwarding port {} → 3001...",
                color::info("▶"),
                port_str
            );

            let actual_port = match sbx(&["ports", name, "--publish", &format!("{port_str}:3001")], false) {
                Ok(_) => opts.port,
                Err(e) if e.contains("address already in use") => {
                    let alt_port = opts.port + 1;
                    println!(
                        "{} Port {port_str} in use, trying {alt_port}...",
                        color::warn("⚠")
                    );
                    match sbx(&["ports", name, "--publish", &format!("{alt_port}:3001")], false) {
                        Ok(_) => alt_port,
                        Err(_) => {
                            eprintln!(
                                "{} Ports {port_str} and {alt_port} both in use. Use --port to specify a free port.",
                                color::error("❌")
                            );
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => return Err(e),
            };

            println!(
                "\n{} {}",
                color::ok("✔"),
                color::bright("CloudCLI is ready!")
            );
            println!(
                "  {} Open {}",
                color::info("→"),
                color::bright(&format!("http://localhost:{actual_port}"))
            );
            println!("\n{}", color::dim("  Manage with:"));
            println!("  {}", color::dim("$ sbx ls"));
            println!("  {}", color::dim(&format!("$ sbx stop {name}")));
            println!("  {}", color::dim(&format!("$ sbx start {name}")));
            println!("  {}", color::dim(&format!("$ sbx rm {name}")));
            println!(
                "\n{}",
                color::dim("  Or install globally: npm install -g @cloudcli-ai/cloudcli")
            );
        }

        _ => {
            show_sandbox_help();
        }
    }

    Ok(())
}
