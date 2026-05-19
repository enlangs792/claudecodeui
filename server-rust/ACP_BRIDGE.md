# ACP Bridge (Rust server)

The Rust server routes WebSocket chat through [Agent Client Protocol](https://agentclientprotocol.com/) agents.

## Enable / disable

| Variable | Default (with `acp-bridge` feature) | Description |
|----------|--------------------------------------|-------------|
| `CLOUDCLI_ACP_BRIDGE` | enabled (`1`) | Set to `0` / `false` to disable ACP routing (WS chat will ack unknown messages only). |
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

## Build

```bash
# Default: ACP bridge (required for /ws chat)
cargo build -p cloudcli-server
```

Legacy CLI `stream-json` agents were removed; the `acp-bridge` feature is required.

## Manual E2E checklist

1. Install prerequisites: Node 18+, and provider CLIs or npx packages above.
2. Start server: `cargo run -p cloudcli-server`
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

## VS Code debugging

Launch configs live in `.vscode/launch.json` (JSONC). Build tasks: `.vscode/tasks.json`.

| Launch / compound | Use when |
|-------------------|----------|
| **Full stack: Mock Claude + Vite** | Offline UI + WS chat (no API keys); uses `mock-acp-agent` as Claude ACP. Open http://localhost:5173 |
| **Full stack: ACP server + Vite** | HMR frontend + real ACP defaults for all installed providers |
| **ACP server + Mock Claude** | Server only; mock agent for `claude-command` |
| **ACP: Claude (real CLI)** | `npx @agentclientprotocol/claude-agent-acp`; set `ANTHROPIC_API_KEY` in repo `.env` |
| **ACP: Gemini** | Requires `gemini` CLI with `--acp` |
| **ACP: Cursor** | Requires `agent acp` (Cursor CLI) |
| **ACP: Codex** | Requires `codex-acp` on `PATH` |

Defaults: Rust API/WebSocket on port **3001**, Vite on **5173** (proxies `/api` and `/ws` to the server). Local DB: `.cloudcli-dev/database.sqlite` (`DATABASE_PATH` in launch env).

Copy `.env.example` → `.env` for `SERVER_PORT`, `VITE_PORT`, and provider API keys. Set `CLOUDCLI_ACP_DEBUG=1` in `.env` or launch `env` to log ACP stdio.

## CI note

Mapper unit tests run in CI. Full bridge E2E uses `mock-acp-agent`. Real `claude-agent-acp` smoke tests need Node 18+, `npx @agentclientprotocol/claude-agent-acp`, and `ANTHROPIC_API_KEY`.
