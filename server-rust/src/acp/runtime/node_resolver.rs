//! Node.js availability checks for Claude ACP agent (npx).

use crate::acp::config;

/// Ensure Node 18+ is available for Claude ACP agents.
pub fn ensure_node_available() -> Result<(), String> {
    let node = config::node_path_override().unwrap_or_else(|| "node".to_string());
    let output = std::process::Command::new(&node)
        .arg("--version")
        .output()
        .map_err(|e| {
            format!(
                "Node.js is required for Claude ACP (npx). Install Node 18+ or set CLOUDCLI_NODE_PATH. ({e})"
            )
        })?;

    if !output.status.success() {
        return Err("Node.js check failed. Install Node 18+.".into());
    }

    let version = String::from_utf8_lossy(&output.stdout);
    let major = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    if major < 18 {
        return Err(format!(
            "Node.js 18+ required (found {version}). Install a newer Node or set CLOUDCLI_NODE_PATH."
        ));
    }

    Ok(())
}
