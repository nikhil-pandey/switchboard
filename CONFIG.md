# Switchboard MCP — Agent Config Reference

This is the authoritative reference for Switchboard MCP configuration. It specifies the exact agent schemas Switchboard MCP accepts, the environment flags it honors, and how each agent type maps into a Codex‑like runtime (executed by a Codex engine) behind the scenes. The root README provides a high‑level overview and examples.

- All discovered agents are exposed as MCP tools with a uniform input schema.
- We do not set model/provider/etc. defaults; the Codex runner supplies its own defaults.

## MCP Tool Input Schema

Every agent tool accepts exactly two required properties:

```json
{
  "task": "<string>",
  "cwd":  "<string>"
}
```

- task: required string — the user’s instruction for the agent.
- cwd: required string — working directory; must be an absolute path.

The tool returns JSON (as a text content block) with fields:

```json
{ "ok": true|false, "output": "<string>" }
```

Errors and debug logs are written to stderr and not included in the payload.

## Runtime Defaults Policy

Switchboard creates/selects a per‑agent Codex profile but only sets fields explicitly provided by the agent config. If a field is absent, it remains unset and Codex’s own defaults apply.

## 1) Switchboard TOML (Codex‑like) Agents (*.toml)

Discovery paths (in order):
- `<workspace>/.agents/*.toml`
- `$SWITCHBOARD_HOME/agents/*.toml` (defaults to `$HOME/.switchboard`)
- `$HOME/.agents/*.toml`

Top‑level schema:
- name: string (required)
- description: string (optional)
- tags: array<string> or string (optional). If a string, it is split on commas and whitespace; items are trimmed and empties removed.
- instructions_file: string path (optional)
- instructions: string (optional) — used if `instructions_file` is not set
- tools: array<string> or string (optional; maps to Codex toggles; see below)
- run: table (optional; forwarded 1:1 to Codex profile fields)
- mcp_servers: table (optional; embeds stdio MCP servers)

Tools → Codex‑like toggles (recognized values):
- plan → include_plan_tool = true
- apply_patch or apply-patch → include_apply_patch_tool = true
- view_image or view-image → include_view_image_tool = true
- web_search or web-search → tools_web_search_request = true

[run] table (all fields optional; forwarded to the Codex runner profile if present):
- model: string
- model_provider: string
- approval_policy: enum (Codex AskForApproval)
- disable_response_storage: bool
- model_reasoning_effort: enum (Codex ReasoningEffort)
- model_reasoning_summary: enum (Codex ReasoningSummary)
- model_verbosity: enum (Codex Verbosity)
- chatgpt_base_url: string
- sandbox_mode: enum (Codex SandboxMode)
- include_plan_tool: bool
- include_apply_patch_tool: bool
- include_view_image_tool: bool
- tools_web_search_request: bool

Runtime mapping for Switchboard TOML:
- Select profile: safe version of `name`.
- base_instructions: from `instructions_file` (if readable) or `instructions` (if non‑empty).
- cwd: from MCP input `cwd`.
- sandbox_mode and include_* toggles: forwarded only if present in `run`.
- mcp_servers: attached (merged with discovered servers).

Enabling built‑in tools
- To turn on Codex built‑ins (plan, apply_patch, view_image, web_search), either:
  - list them in `tools = [...]` (see mapping above), or
  - set the corresponding `[run]` toggles (`include_*`, `tools_web_search_request`).
- If neither is provided, built‑ins remain off.

Example:

```toml
name = "Explain Build Failures"
description = "Diagnose failing builds and propose fixes"
tags = ["triage", "build"]
tools = ["plan", "apply_patch", "web_search"]
instructions_file = ".agents/explain.prompt.md"

[run]
model = "gpt-4o-mini"            # optional; omit to use Codex defaults
model_provider = "openai"         # optional
tools_web_search_request = true    # optional toggle

[mcp_servers.memory]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-memory"]
# env = { FOO = "BAR" }           # optional
```

## 2) Anthropic/Claude Agents (*.agent.md)

Discovery paths (in order):
- `<workspace>/.claude/agents/*.agent.md`
- `$SWITCHBOARD_HOME/agents/*.agent.md` (defaults to `$HOME/.switchboard`)
- `$HOME/.claude/agents/*.agent.md`

Schema: YAML front‑matter followed by the instruction body.

```yaml
---
name: <string>                # required
description: <string>         # optional
tools: <string>|<list>        # optional (mapping below)
tags: <string>|<list>         # optional (string is comma‑separated; spaces preserved)
model: <string>               # optional; maps to run.model
provider/modelProvider: <string> # optional; maps to run.model_provider
---
<body>                        # becomes the instructions
```

Tools parsing:
- If a single string, split on commas and whitespace.
- If a list, use items as given.
- Parsed tools become bare MCP tool refs; provider mapping then translates known tools to Codex toggles (or namespaced MCP tools) when mapping is enabled.

Anthropic tool mapping → Codex‑like toggles:
- Edit, MultiEdit, Write, NotebookEdit → include_apply_patch_tool = true
- WebSearch, WebFetch → tools_web_search_request = true
- TodoWrite → include_plan_tool = true
- Bash, Glob, Grep, Read, BashOutput, KillBash → consumed as terminal access (no toggle; provided by default)

- Optional model/provider:
- `model: <string>` maps to `run.model`.
- `provider:` or `modelProvider:` maps to `run.model_provider`.
- This format does not carry a `[run]` table otherwise; mapping may still set include_* / web_search toggles internally.
- Tag strings are split on commas only to allow multi‑word tags (e.g., "deep research").

## 3) VS Code Chat Modes (*.chatmode.md)

Discovery paths (in order):
- `<workspace>/.github/chatmodes/*.chatmode.md`
- `$SWITCHBOARD_HOME/chatmodes/*.chatmode.md` (defaults to `$HOME/.switchboard`)
- `$HOME/.chatmodes/*.chatmode.md`

Schema: YAML front‑matter followed by the instruction body.

```yaml
---
name: <string>                # optional; derived from filename if omitted
description: <string>         # required
tools: <string>|<list>        # optional; bare or "server::tool"
model: <string>               # optional, metadata only
provider/modelProvider: <string> # optional, metadata only
tags: <string>|<list>         # optional
---
<body>
```

Tools parsing:
- Bare entries (e.g., `edit`, `search`) are mapped via the provider table below when mapping is enabled.
- Namespaced entries (e.g., `memory::memory`) pin to that MCP server/tool.
- Parsed tools become MCP tool refs; mapping may convert bare ones to Codex toggles and optionally inject default servers.

VS Code tool mapping → Codex‑like toggles:
- edit, new → include_apply_patch_tool = true
- search, fetch, githubRepo → tools_web_search_request = true
- runCommands → consumed as terminal access (no toggle)
- openSimpleBrowser and other placeholders remain unmapped.

Default server injection (when mapping is enabled):
- Bare `memory` → namespaced `memory::memory` and inject a stdio server: `npx -y @modelcontextprotocol/server-memory`.

Optional model/provider
- VS Code front‑matter may include `model: <string>`; it maps to `run.model`.
- It may also include `provider:` or `modelProvider:`; it maps to `run.model_provider`.

## MCP Servers: Discovery, Enumeration, Attachment

Discovery and embedding:
- Switchboard can discover stdio MCP servers from provider configs (VS Code, Claude, Cursor) and merge them with any servers embedded in Codex TOML.
- Embedded server schema (stdio only):

```toml
[mcp_servers.<key>]
command = "<string>"
args = ["<string>", ...]     # optional
env = { KEY = "VALUE", ... } # optional
```

Referencing tools:
- Bare tool refs rely on mapping or (if enabled) enumeration to find matching servers.
- Namespaced refs use `server_key::tool` and pin to that server.

Enumeration and gating (optional):
- When enabled, Switchboard enumerates candidate servers and injects only those that actually expose the referenced tools. Policy on ambiguous matches is controlled by a flag (see below).

Tool exposure
- Once a server is attached, the Codex runner currently does not support per‑tool enable/disable. All tools exposed by that MCP server are available to the agent. Use server‑level selection (referenced‑only or enumerated) to constrain what is attached.

Non‑stdio servers (HTTP/SSE) via proxy
- Embedded servers must be stdio because the Codex runner does not attach non‑stdio servers directly.
- To use an HTTP/SSE‑only MCP server from an agent, bridge it to stdio with a proxy such as [mcp-proxy](https://github.com/sparfenyuk/mcp-proxy).

Example (proxying a Streamable HTTP server):

```toml
[mcp_servers.deepwiki]
command = "mcp-proxy"
args = ["--transport", "streamablehttp", "https://mcp.deepwiki.com/mcp"]

# For SSE endpoints (default transport):
# args = ["https://mcp.deepwiki.com/sse"]

# Optional auth:
# env = { API_ACCESS_TOKEN = "<token>" }
```


## Runtime Mapping (Codex‑powered)

What Switchboard sets:
- config_profile: safe(name)
- cwd: from MCP input
- base_instructions: from instructions (file or inline) if non‑empty
- sandbox_mode and include_* / web_search toggles: forwarded only if present
- MCP servers: attached from discovery + embedded `[mcp_servers]`

What Switchboard never defaults (the Codex runner applies its own defaults):
- model, model_provider, approval_policy, disable_response_storage
- model_reasoning_effort, model_reasoning_summary, model_verbosity
- chatgpt_base_url

## Server Flags (Environment Variables)

Transport and logging:
- `TRANSPORT`: `stdio` (default) or `http`
- `HOST`, `PORT`: when `TRANSPORT=http`
- `TRACING_JSON`, `TRACING_COMPACT`, `TRACING_PRETTY`, `TRACING_FILTER` (alias for `RUST_LOG`), `RUST_LOG`

Discovery and directories:
- `WORKSPACE_DIR`
- `AGENTS_ENABLE_CODEX`, `AGENTS_ENABLE_ANTHROPIC`, `AGENTS_ENABLE_VSCODE`
- `AGENTS_DIRS`, `ANTHROPIC_AGENTS_DIRS`, `VSCODE_CHATMODES_DIRS`
- `AGENTS_PREFIX_CODEX`, `AGENTS_PREFIX_ANTHROPIC`, `AGENTS_PREFIX_VSCODE`
- `AGENTS_FILTER` (by name/safe name/tag)

MCP servers and mapping:
- `AGENTS_MCP_DISCOVERY` (discover stdio servers)
- `VSCODE_USER_MCP` (path to VS Code user `mcp.json`)
- `AGENTS_MCP_LIMIT_REFERENCED` (attach only referenced servers)
- `AGENTS_MCP_ENUMERATE` (enumerate and gate by tool availability)
- `AGENTS_MCP_ENUM_TIMEOUT_MS`, `AGENTS_MCP_MAX_SERVERS`, `AGENTS_MCP_ENUM_STRICT`, `AGENTS_MCP_ENUM_FALLBACK` = `none|all`
- `AGENTS_TOOLMAP_ENABLE` (provider tool mapping)
- `AGENTS_TOOLMAP_ALLOW_CUSTOM_SERVERS` (permit injected servers like memory)

Model mapping:
- `AGENTS_MODEL_MAP_ENABLE` (enable model mapping; default true)
- `AGENTS_MODEL_MAP_FILE` (path to a mapping TOML). If empty, defaults to `<workspace>/.agents/model-map.toml`.
- `AGENTS_MODEL_MAP_STRICT` (warn on unknown tokens and leave unchanged)
- `AGENTS_MODEL_MAP_OVERRIDE_PROVIDER` (allow mapping to override user-specified provider)
- `AGENTS_MODEL_MAP_NORMALIZE_PROVIDER` (normalize provider aliases like `Claude` → `anthropic`)

Model mapping file format (TOML):

```toml
[[mappings]]
token = "GPT-4.1"
to_model = "gpt-4.1"
to_provider = "openai"

[[mappings]]
token = "sonnet"
to_model = "claude-3-5-sonnet-latest"
to_provider = "anthropic"

[[mappings]]
token = "o3"
to_model = "o3"
to_provider = "openai"
aliases = ["O3", "OpenAI O3"]

[provider_aliases]
OpenAI = "openai"
Anthropic = "anthropic"
Claude = "anthropic"
```

Built-in defaults
- If no mapping file is provided, Switchboard includes sensible defaults:
  - Anthropic: `sonnet` → `gpt-5` (openai), `opus` → `gpt-5` (openai), `haiku` → `gpt-5-mini` (openai)
  - VS Code: `Claude Sonnet 3.5` → `gpt-5` (openai), `Gemini 2.5 Pro` → `gpt-5` (openai),
    `GPT-4.1` → `gpt-4.1` (openai), `GPT-4o` → `gpt-4o` (openai), `GPT-5 mini (Preview)`/`GPT-5 mini` → `gpt-5-mini` (openai),
    `o3-mini` → `o3-mini` (openai), `Auto` → `gpt-5` (openai)
  - You can override or extend these by adding a `model-map.toml` in `.agents/`.

Notes:
- Mapping is enabled by default. Unknown vendor tools remain explicit bare refs until you choose a mapping or provide namespaced refs.
