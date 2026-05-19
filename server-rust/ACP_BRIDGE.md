# ACP Bridge (Rust server)

The Rust server can route WebSocket chat through [Agent Client Protocol](https://agentclientprotocol.com/) agents instead of parsing CLI `stream-json` output.

## Enable / disable

| Variable | Default (with `acp-bridge` feature) | Description |
|----------|--------------------------------------|-------------|
| `CLOUDCLI_ACP_BRIDGE` | enabled (`1`) | Set to `0` / `false` to use legacy CLI agents (requires `legacy-cli-agents` feature at build time). |
| `CLOUDCLI_ACP_DEBUG` | off | Log ACP stdio lines. |
| `CLOUDCLI_NODE_PATH` | `node` | Node binary for Claude (`npx` agent). |
| `CLOUDCLI_MANAGED_NODE` | off | Reserved for bundled Node (Phase 4). |

Per-provider overrides:

| Variable | Default command |
|----------|-----------------|
| `CLOUDCLI_ACP_CLAUDE_CMD` | `npx -y @agentclientprotocol/claude-agent-acp` |
| `CLOUDCLI_ACP_GEMINI_CMD` | `gemini --acp` |
| `CLOUDCLI_ACP_CURSOR_CMD` | `agent acp` |
| `CLOUDCLI_ACP_CODEX_CMD` | `codex-acp` |
| `CLOUDCLI_ACP_<PROVIDER>_ENABLED` | `1` | Set to `0` to disable a provider on the bridge. |

## Build features

```bash
# Default: ACP bridge only
cargo build -p cloudcli-server

# Legacy CLI stream-json agents (rollback)
cargo build -p cloudcli-server --no-default-features --features legacy-cli-agents

# Both paths
cargo build -p cloudcli-server --features acp-bridge,legacy-cli-agents
```

## Manual E2E checklist

1. Install prerequisites: Node 18+, and provider CLIs or npx packages above.
2. Start server: `CLOUDCLI_ACP_BRIDGE=1 cargo run -p cloudcli-server`
3. Connect frontend to `/ws` with auth token.
4. **Claude:** send `claude-command` with `options.cwd` — expect `session_created`, `stream_delta` / `text`, `complete`.
5. **Permissions:** tool that requires approval — expect `permission_request`; reply with `claude-permission-response`.
6. **Abort:** `abort-session` with `sessionId` + `provider`.
7. **Status:** `check-session-status`, `get-active-sessions`, `get-pending-permissions`.
8. Repeat for gemini / cursor / codex when binaries are installed.

## Automated E2E (no API keys)

```bash
cd server-rust
CARGO_TARGET_DIR=./target cargo test --test acp_bridge_e2e
```

Uses the `mock-acp-agent` binary (`tests/fixtures/mock_acp_agent.rs`) as `CLOUDCLI_ACP_CLAUDE_CMD` to verify bridge → ACP → `stream_delta` / `complete` and `abort-session`.

## Manual WS smoke (mock agent)

```bash
export CLOUDCLI_ACP_CLAUDE_CMD="$PWD/target/debug/mock-acp-agent"
CARGO_TARGET_DIR=./target cargo run --bin cloudcli-server
# Connect to /ws?token=... and send claude-command (see checklist above).
```

## CI note

Mapper unit tests run in CI. Full bridge E2E uses `mock-acp-agent`. Real `claude-agent-acp` smoke tests need Node 18+, `npx @agentclientprotocol/claude-agent-acp`, and `ANTHROPIC_API_KEY`.
