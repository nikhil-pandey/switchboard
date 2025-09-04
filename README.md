# Switchboard MCP ⚡️🔌 — Codex‑Powered Subagents For Any MCP Client

Switchboard MCP is an adapter MCP server. It discovers agents defined across ecosystems — Switchboard TOML (a Codex‑like schema), Claude/Anthropic front‑matter, and VS Code Chat Modes — normalizes them into a Codex‑like agent shape, executes them with a Codex engine, and exposes them as plain MCP tools. You speak MCP; we do the Codex work behind the scenes.

• BYOA (Bring Your Own Agents) • Simple `{ task, cwd }` input • stdio or HTTP/SSE transport

Works with any MCP client: VS Code, Claude Code, Cursor, Codex CLI, MCP Inspector, and more.

Note on terminology
- “Codex‑like” means our agent/tool conventions are inspired by Codex (e.g., `apply_patch`, `plan`, `web_search`), but are not a compatibility promise or a drop‑in for Codex configs.
- We discover Switchboard TOML (our Codex‑like format), not Codex TOML. Internally we run agents with a Codex engine.

How to think about it
- Adapter: Discover → Normalize (Codex‑like) → Execute (Codex) → Expose (MCP)
- Subagents: Each discovered agent becomes an MCP tool you can call from any client.
- Clients: Works in any MCP host; Codex is one of many.

## 🏁 Quick Start

- Install (one‑liner):
  - `cargo install --git https://github.com/nikhil-pandey/switchboard --locked`
- Add to your MCP client (copy one):

```json
// VS Code (project-level .vscode/mcp.json)
{
  "servers": {
    "switchboard": { "command": "switchboard-mcp", "args": [], "env": { "RUST_LOG": "info" } }
  }
}
```

```json
// Claude Code (global config)
{
  "mcpServers": {
    "switchboard": { "command": "switchboard-mcp", "args": [], "env": { "RUST_LOG": "info" } }
  }
}
```

```sh
# Claude Code (CLI)
claude mcp add switchboard --transport stdio -- switchboard-mcp
```

```json
// Cursor (global ~/.cursor/mcp.json or project .cursor/mcp.json)
{
  "mcpServers": {
    "switchboard": { "command": "switchboard-mcp", "args": [], "env": { "RUST_LOG": "info" } }
  }
}
```

```sh
# Codex CLI (recommended): inline per-run config (avoid global config.toml)
# global config can accidentally create infinite recursion if
# switchboard discovers itself; prefer ephemeral -c flags instead for now
# until we have a better solution.
#
# Also, Codes does not automatically forward env variables to MCP child processes;
# set them explicitly in the server env for the spawned process.
codex \
  -c mcp_servers.switchboard.command=switchboard-mcp \
  -c "mcp_servers.switchboard.env={OPENAI_API_KEY=\"${OPENAI_API_KEY}\",TRACING_FILTER=\"info\"}"
```

### HTTP Mode (MCP over HTTP/SSE)

You can also run Switchboard as an HTTP MCP server (SSE-based) and point HTTP-capable MCP hosts at it.

- Start the server over HTTP:
  - `TRANSPORT=http HOST=127.0.0.1 PORT=8081 switchboard-mcp`
  - Optional: `PING_SECS=5` (SSE ping), `HTTP_JSON=false` (enable JSON response mode only for debugging/clients that expect JSON).

- Configure your MCP host to use HTTP:

```json
// VS Code (.vscode/mcp.json)
{
  "servers": {
    "switchboard": { "transport": "http", "url": "http://127.0.0.1:8081" }
  }
}
```

```json
// Claude Code (global config)
{
  "mcpServers": {
    "switchboard": { "transport": "http", "url": "http://127.0.0.1:8081" }
  }
}
```

```json
// Cursor (global ~/.cursor/mcp.json or project .cursor/mcp.json)
{
  "mcpServers": {
    "switchboard": { "transport": "http", "url": "http://127.0.0.1:8081" }
  }
}
```

### Note: Embedded MCP Servers Must Be stdio

Switchboard itself can run over stdio or HTTP/SSE as a server. However, embedded/attached MCP servers inside agents (the `[mcp_servers.*]` blocks) must be stdio-only right now because the underlying Codex runner does not attach non-stdio servers directly.

If you need to use an HTTP/SSE-only MCP server with an agent, use a stdio bridge like [mcp-proxy](https://github.com/sparfenyuk/mcp-proxy) to proxy it:

```toml
[mcp_servers.deepwiki]
command = "mcp-proxy"
# For Streamable HTTP endpoints
args = ["--transport", "streamablehttp", "https://mcp.deepwiki.com/mcp"]
# For SSE endpoints (default transport)
# args = ["https://mcp.deepwiki.com/sse"]

# Optional auth via env or headers
# env = { API_ACCESS_TOKEN = "<token>" }
# Or pass headers with repeated -H/--headers flags if your client supports args arrays
```

### Auto‑Discovery & Paths (BYOA)
- Drop your existing agents and we auto‑load them as Switchboard agent tools — no rewrites:
  - Switchboard TOML (Codex‑like): `./.agents/`, `~/.agents/`, and `~/.switchboard/agents/` (also `<workspace>/.switchboard/agents` if `$HOME` is unset)
  - Anthropic agents: `./.claude/agents/`, `~/.claude/agents/`, and `~/.switchboard/agents/`
  - VS Code chat modes: `./.github/chatmodes/`, `~/.chatmodes/`, and `~/.switchboard/chatmodes/`
- Tools map to Switchboard’s Codex‑like built‑ins where sensible; attached MCP servers expose their full toolsets.
- Verify: start your client, confirm tools are listed, call with `{ task, cwd }`.
- Optional: add `.agents/model-map.toml` to normalize model/provider tokens across formats.

### Verify With MCP Inspector (optional)
- UI (stdio): `npx -y @modelcontextprotocol/inspector switchboard-mcp` → open UI, list tools, call with `{ task, cwd }` (cwd must be absolute).
- CLI (stdio): `npx -y @modelcontextprotocol/inspector --cli switchboard-mcp --method tools/list`
- Call a tool (example):
  - `npx -y @modelcontextprotocol/inspector --cli switchboard-mcp --method tools/call --tool-name agent_<safe-name> --tool-arg task='Explain the failing build' --tool-arg cwd="$PWD"`
- HTTP: start `TRANSPORT=http HOST=127.0.0.1 PORT=8081 switchboard-mcp`, then either use the UI and set transport to SSE with URL `http://127.0.0.1:8081`, or CLI: `npx -y @modelcontextprotocol/inspector --cli http://127.0.0.1:8081 --method tools/list`.

## 🧪 Call Any Agent Tool

- Input schema: `{ "task": "<string>", "cwd": "<string>" }` (both required)
- Result payload: `{ "ok": true|false, "output": "<string>" }`
- All logs go to stderr; stdout is reserved for JSON‑RPC.

See CONFIG.md for the full schema, tool mapping, and MCP server behavior.

## 🧭 Provider Tool Mapping (defaults)

- VS Code → Codex‑like: `edit`/`new` → apply_patch, `search`/`fetch`/`githubRepo` → web_search, `runCommands` → terminal (no toggle)
- Claude/Anthropic → Codex‑like: `Edit`/`MultiEdit`/`Write`/`NotebookEdit` → apply_patch, `WebSearch`/`WebFetch` → web_search, `TodoWrite` → plan
- Unknown vendor tools remain explicit. Attached MCP servers expose their full toolsets.

## 🧱 Model Mapping (optional)

- Default mapping file: `.agents/model-map.toml` (case‑insensitive tokens).
- Built‑in defaults cover Anthropic “sonnet/opus/haiku” and common VS Code tokens (e.g., “Claude Sonnet 3.5”, “GPT‑4o”, “Auto”).
- Controls normalization of `run.model` and `run.model_provider`. Flags: `AGENTS_MODEL_MAP_*`. See CONFIG.md for format.

## ⚙️ Configuration (at a glance)

- Transport/logging: `TRANSPORT=stdio|http`, `HOST`, `PORT`, `RUST_LOG`, `TRACING_JSON|COMPACT|PRETTY`
- Discovery/dirs: `WORKSPACE_DIR`, `AGENTS_ENABLE_*`, `*_DIRS`, `AGENTS_FILTER`, `AGENTS_PREFIX_*`
- MCP servers: `AGENTS_MCP_DISCOVERY`, `VSCODE_USER_MCP`, `AGENTS_MCP_ENUMERATE`, `AGENTS_MCP_LIMIT_REFERENCED`, `AGENTS_MCP_ENUM_*`
- Tool mapping: `AGENTS_TOOLMAP_ENABLE`, `AGENTS_TOOLMAP_ALLOW_CUSTOM_SERVERS`
- Model mapping: `AGENTS_MODEL_MAP_*` (see CONFIG.md)

Defaults are chosen to “just work” locally. See CONFIG.md for the full reference.

## 🛠️ Development
- Edition: Rust 2024
- Build/test/lint: `cargo test`, `cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`

### Tip: MCP Inception
- Yes, you can run Switchboard inside Switchboard. It’s like a turducken of agents — a dream within a dream, but with filters.
- Create a Switchboard TOML (Codex‑like) agent that embeds Switchboard and scopes it with `AGENTS_FILTER`:

```toml
# ./.agents/switchboard-scoped.toml
name = "Switchboard (Scoped)"
description = "A Switchboard agent that only knows about 'docs' and 'lint' agents"
tools = ["plan", "apply_patch"]

[mcp_servers.switchboard]
command = "switchboard-mcp"
args = []
env = { AGENTS_FILTER = "docs lint" }
```

- Want to go deeper? Add another agent that points to Switchboard again with an even narrower `AGENTS_FILTER` (e.g., just `docs`). Congrats, you now have a switchboard agent that calls a switchboard agent that only calls… you get it.
- Verify with the Inspector: list tools, find your `agent_switchboard_scoped` tool, and call it with `{ task, cwd }`. If the room starts spinning, step away from the recursion.

## 🤝 Contributing
- Issues and PRs welcome. Keep changes focused; include tests where meaningful.

## 📄 License
- MIT
