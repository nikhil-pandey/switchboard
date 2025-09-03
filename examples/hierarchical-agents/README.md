# Hierarchical Agents — Supervisor → Routers → Specialists

This example shows how a single user call routes through a hierarchy:
- Supervisor coordinates the review, merges diffs, and runs verification.
- Routers classify changes by stack (Rust/.NET/Frontend) and fan out to specialists.
- Specialists propose minimal diffs and tests for specific concerns (correctness, maintainability, tests, error-handling).

Switchboard MCP exposes child agents as plain MCP tools to parents. Each parent embeds a Switchboard server configured via environment to only discover its children.

## Architecture (Flow)

```mermaid
flowchart TD
    U[User and MCP Client] -->|calls tool: agent_review-supervisor| S[Supervisor Agent]
    S -->|embedded MCP: switchboard| SW1[Switchboard - routers]
    SW1 --> RUST[router-rust]
    SW1 --> DOTNET[router-dotnet]
    SW1 --> FE[router-frontend]

    RUST -->|embedded MCP: switchboard| SW2R[Switchboard - specialists]
    DOTNET -->|embedded MCP: switchboard| SW2D[Switchboard - specialists]
    FE -->|embedded MCP: switchboard| SW2F[Switchboard - specialists]

    SW2R --> CORR[correctness-reviewer]
    SW2R --> MAINT[maintainability-reviewer]
    SW2R --> TESTS[tests-reviewer]
    SW2R --> ERR[error-handling-reviewer]

    SW2D --> CORR
    SW2D --> MAINT
    SW2D --> TESTS
    SW2D --> ERR

    SW2F --> CORR
    SW2F --> MAINT
    SW2F --> TESTS
    SW2F --> ERR

    subgraph Verify
      V1[cargo fmt/clippy/test]
      V2[dotnet format/build/test]
      V3[pnpm lint/typecheck/test/build]
    end

    CORR --> M[Supervisor: Patch Merge]
    MAINT --> M
    TESTS --> M
    ERR --> M

    M --> V1
    M --> V2
    M --> V3
    V1 -->|logs| S
    V2 -->|logs| S
    V3 -->|logs| S
    S -->|unified patch + report| U
```

## Sequence (Routing & Verification)

```mermaid
sequenceDiagram
    participant U as User
    participant S as Supervisor
    participant SW1 as Switchboard routers
    participant R as Router stack
    participant SW2 as Switchboard specialists
    participant SP as Specialist

    U->>S: task + cwd
    S->>S: git diff classify Rust .NET FE
    S->>SW1: list tools filtered routers
    S->>R: send file slices and goals
    R->>SW2: list tools filtered specialists
    R->>SP: review packet focused context
    SP-->>R: findings and minimal diff and verify cmds
    R-->>S: aggregated findings
    S->>S: merge non-overlapping diffs
    par Verify per stack
      S->>S: cargo fmt/clippy/test
      S->>S: dotnet format/build/test
      S->>S: pnpm lint/typecheck/test/build
    end
    S-->>U: summary and unified patch set
```

## Wiring via Switchboard MCP

- Supervisor (parent) embeds Switchboard to expose routers only:

```toml
[mcp_servers.switchboard]
command = "switchboard-mcp"
args = []
env = {
  AGENTS_DIRS = "examples/hierarchical-agents/routers",
  AGENTS_FILTER = "router-rust router-dotnet router-frontend"
}
```

- Routers embed Switchboard to expose specialists only:

```toml
[mcp_servers.switchboard]
command = "switchboard-mcp"
args = []
env = {
  AGENTS_DIRS = "examples/hierarchical-agents/specialists",
  AGENTS_FILTER = "correctness-reviewer maintainability-reviewer tests-reviewer error-handling-reviewer"
}
```

Notes
- Parents call children as MCP tools. No extra tools beyond terminal + apply_patch.
- Embedded servers must be stdio — see project README for HTTP/SSE proxying if needed.
- Adjust AGENTS_DIRS/AGENTS_FILTER to include/exclude child agents by path/name/tag.

## Files
- supervisor: examples/hierarchical-agents/supervisor/review-supervisor.toml
- routers: examples/hierarchical-agents/routers/router-{rust,dotnet,frontend}.toml
- specialists: examples/hierarchical-agents/specialists/specialist-*.toml

## Quick Verify with MCP Inspector (npx)

- List routers as tools exposed by Switchboard:

```sh
AGENTS_DIRS="examples/hierarchical-agents/routers" \
AGENTS_FILTER="router-rust router-dotnet router-frontend" \
npx -y @modelcontextprotocol/inspector --cli switchboard-mcp --method tools/list
```

- List specialists as tools exposed by Switchboard:

```sh
AGENTS_DIRS="examples/hierarchical-agents/specialists" \
AGENTS_FILTER="correctness-reviewer maintainability-reviewer tests-reviewer error-handling-reviewer" \
npx -y @modelcontextprotocol/inspector --cli switchboard-mcp --method tools/list
```

- List the supervisor agent tool (so you can call it):

```sh
AGENTS_DIRS="examples/hierarchical-agents/supervisor" \
npx -y @modelcontextprotocol/inspector --cli switchboard-mcp --method tools/list
```

- Call the supervisor agent (dry run example):

```sh
AGENTS_DIRS="examples/hierarchical-agents/supervisor" \
npx -y @modelcontextprotocol/inspector --cli switchboard-mcp \
  --method tools/call \
  --tool-name agent_review-supervisor \
  --tool-arg task='Review the latest diff and plan routing (dry run)' \
  --tool-arg cwd="$PWD"
```
