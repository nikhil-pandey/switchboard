mod codex_runner;
mod config;
mod handler;
mod loader;
mod mcp;
mod model;
mod modelmap;
mod parser;
mod toolmap;

use std::time::Duration;

use env_flags::env_flags;
use once_cell::sync::OnceCell;
use rust_mcp_sdk::error::SdkResult;
use rust_mcp_sdk::mcp_server::{
    HyperServerOptions, ServerRuntime, hyper_server_core, server_runtime_core,
};
use rust_mcp_sdk::schema::{
    Implementation, InitializeResult, LATEST_PROTOCOL_VERSION, ServerCapabilities,
    ServerCapabilitiesTools,
};
use rust_mcp_sdk::{McpServer, StdioTransport, TransportOptions};

use crate::handler::AgentsServerHandler;
use crate::loader::{default_settings, prepare_all};

fn init_tracing() {
    env_flags! {
        /// Tracing filter, e.g. "info", "debug", or targets format.
        RUST_LOG: &str = "info";
        /// Preferred filter env (alias). If set, overrides RUST_LOG.
        TRACING_FILTER: &str = "";
        /// Pretty formatting for logs (ignored if TRACING_JSON=true). Prefer compact unless explicitly set.
        TRACING_PRETTY: bool = false;
        /// Compact single-line formatting for logs (ignored if TRACING_JSON=true)
        TRACING_COMPACT: bool = true;
        /// JSON formatting for logs
        TRACING_JSON: bool = false;
        /// If true, also log to file under <SWITCHBOARD_HOME>/logs or LOG_DIR
        LOG_TO_FILE: bool = true;
        /// Optional explicit log directory (absolute). Defaults to <SWITCHBOARD_HOME>/logs
        LOG_DIR: &str = "";
        /// Switchboard home directory (absolute). Defaults to $HOME/.switchboard
        SWITCHBOARD_HOME: &str = "";
    }

    use tracing_subscriber::{EnvFilter, layer::SubscriberExt, prelude::*};

    // Determine Switchboard home
    let sb_home = if !(*SWITCHBOARD_HOME).is_empty() {
        std::path::PathBuf::from((*SWITCHBOARD_HOME).to_string())
    } else if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".switchboard")
    } else {
        // Fallback: current dir /.switchboard
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(".switchboard")
    };

    // Load user config (optional) and allow it to influence tracing defaults when env is not set
    let user_cfg = crate::config::load_user_config(&sb_home).ok().flatten();
    let env_set = |k: &str| std::env::var_os(k).is_some();

    // Support TRACING_FILTER as primary; fall back to RUST_LOG; then user config.
    let mut rust_log = if !(*TRACING_FILTER).is_empty() {
        (*TRACING_FILTER).to_string()
    } else {
        (*RUST_LOG).to_string()
    };
    let mut tracing_json = *TRACING_JSON;
    let mut tracing_compact = *TRACING_COMPACT;
    let mut tracing_pretty = *TRACING_PRETTY;
    let mut log_to_file = *LOG_TO_FILE;
    let mut log_dir: Option<std::path::PathBuf> = if !(*LOG_DIR).is_empty() {
        Some(std::path::PathBuf::from((*LOG_DIR).to_string()))
    } else {
        None
    };

    if let Some(cfg) = user_cfg.as_ref().and_then(|c| c.logging.as_ref()) {
        if !(env_set("TRACING_FILTER") || env_set("RUST_LOG"))
            && let Some(level) = cfg.level.as_ref()
        {
            rust_log = level.clone();
        }
        if !env_set("TRACING_JSON")
            && let Some(v) = cfg.json
        {
            tracing_json = v;
        }
        if !env_set("TRACING_COMPACT")
            && let Some(v) = cfg.compact
        {
            tracing_compact = v;
        }
        if !env_set("TRACING_PRETTY")
            && let Some(v) = cfg.pretty
        {
            tracing_pretty = v;
        }
        if !env_set("LOG_TO_FILE")
            && let Some(v) = cfg.to_file
        {
            log_to_file = v;
        }
        if !env_set("LOG_DIR")
            && let Some(dir) = cfg.dir.as_ref()
        {
            log_dir = Some(std::path::PathBuf::from(dir));
        }
    }

    // Build filter from derived level
    let filter = EnvFilter::try_new(rust_log).unwrap_or_else(|_| EnvFilter::new("info"));

    // Choose formatter base (we'll select specific style below)
    let base = tracing_subscriber::fmt::layer()
        .with_file(false)
        .with_line_number(false)
        .with_target(true)
        .with_ansi(true)
        // Always write logs to stderr to avoid contaminating stdio JSON-RPC.
        .with_writer(std::io::stderr);
    // Optional file logging layer
    static FILE_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();
    let reg = tracing_subscriber::registry().with(filter);
    // Build stderr + optional file layers per selected format, then init
    if tracing_json {
        let stderr_layer = base.json();
        if log_to_file {
            let dir = log_dir.unwrap_or_else(|| sb_home.join("logs"));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!("failed to create log dir {}: {}", dir.display(), e);
                let subscriber = reg.with(stderr_layer);
                if let Err(e) = subscriber.try_init() {
                    tracing::debug!("tracing already set: {:?}", e);
                }
            } else {
                let appender = tracing_appender::rolling::daily(dir, "switchboard-mcp.log");
                let (nb, guard) = tracing_appender::non_blocking(appender);
                let _ = FILE_GUARD.set(guard);
                let file_layer = tracing_subscriber::fmt::layer()
                    .with_file(false)
                    .with_line_number(false)
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(nb)
                    .json();
                let subscriber = reg.with(stderr_layer).with(file_layer);
                if let Err(e) = subscriber.try_init() {
                    tracing::debug!("tracing already set: {:?}", e);
                }
            }
        } else {
            let subscriber = reg.with(stderr_layer);
            if let Err(e) = subscriber.try_init() {
                tracing::debug!("tracing already set: {:?}", e);
            }
        }
    } else if tracing_compact {
        let stderr_layer = base.compact();
        if log_to_file {
            let dir = log_dir.unwrap_or_else(|| sb_home.join("logs"));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!("failed to create log dir {}: {}", dir.display(), e);
                let subscriber = reg.with(stderr_layer);
                if let Err(e) = subscriber.try_init() {
                    tracing::debug!("tracing already set: {:?}", e);
                }
            } else {
                let appender = tracing_appender::rolling::daily(dir, "switchboard-mcp.log");
                let (nb, guard) = tracing_appender::non_blocking(appender);
                let _ = FILE_GUARD.set(guard);
                let file_layer = tracing_subscriber::fmt::layer()
                    .with_file(false)
                    .with_line_number(false)
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(nb)
                    .compact();
                let subscriber = reg.with(stderr_layer).with(file_layer);
                if let Err(e) = subscriber.try_init() {
                    tracing::debug!("tracing already set: {:?}", e);
                }
            }
        } else {
            let subscriber = reg.with(stderr_layer);
            if let Err(e) = subscriber.try_init() {
                tracing::debug!("tracing already set: {:?}", e);
            }
        }
    } else if tracing_pretty {
        let stderr_layer = base.pretty();
        if log_to_file {
            let dir = log_dir.unwrap_or_else(|| sb_home.join("logs"));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!("failed to create log dir {}: {}", dir.display(), e);
                let subscriber = reg.with(stderr_layer);
                if let Err(e) = subscriber.try_init() {
                    tracing::debug!("tracing already set: {:?}", e);
                }
            } else {
                let appender = tracing_appender::rolling::daily(dir, "switchboard-mcp.log");
                let (nb, guard) = tracing_appender::non_blocking(appender);
                let _ = FILE_GUARD.set(guard);
                let file_layer = tracing_subscriber::fmt::layer()
                    .with_file(false)
                    .with_line_number(false)
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(nb)
                    .pretty();
                let subscriber = reg.with(stderr_layer).with(file_layer);
                if let Err(e) = subscriber.try_init() {
                    tracing::debug!("tracing already set: {:?}", e);
                }
            }
        } else {
            let subscriber = reg.with(stderr_layer);
            if let Err(e) = subscriber.try_init() {
                tracing::debug!("tracing already set: {:?}", e);
            }
        }
    } else {
        let stderr_layer = base;
        if log_to_file {
            let dir = log_dir.unwrap_or_else(|| sb_home.join("logs"));
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!("failed to create log dir {}: {}", dir.display(), e);
                let subscriber = reg.with(stderr_layer);
                if let Err(e) = subscriber.try_init() {
                    tracing::debug!("tracing already set: {:?}", e);
                }
            } else {
                let appender = tracing_appender::rolling::daily(dir, "switchboard-mcp.log");
                let (nb, guard) = tracing_appender::non_blocking(appender);
                let _ = FILE_GUARD.set(guard);
                let file_layer = tracing_subscriber::fmt::layer()
                    .with_file(false)
                    .with_line_number(false)
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(nb);
                let subscriber = reg.with(stderr_layer).with(file_layer);
                if let Err(e) = subscriber.try_init() {
                    tracing::debug!("tracing already set: {:?}", e);
                }
            }
        } else {
            let subscriber = reg.with(stderr_layer);
            if let Err(e) = subscriber.try_init() {
                tracing::debug!("tracing already set: {:?}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> SdkResult<()> {
    // Initialize tracing early
    init_tracing();

    env_flags! {
        /// Transport: "stdio" (default) or "http"
        TRANSPORT: &str = "stdio";
        /// Host for HTTP transport
        HOST: &str = "127.0.0.1";
        /// Port for HTTP transport
        PORT: u16 = 8081;
        /// Ping interval for HTTP SSE
        PING_SECS: u64 = 5;
        /// Enable JSON response mode for HTTP
        HTTP_JSON: bool = false;
        /// Workspace directory base for Switchboard MCP. If empty, defaults to the current execution directory.
        WORKSPACE_DIR: &str = "";
        /// Enable loaders (all default to true)
        AGENTS_ENABLE_CODEX: bool = true;
        AGENTS_ENABLE_ANTHROPIC: bool = true;
        AGENTS_ENABLE_VSCODE: bool = true;
        /// Directories (comma-separated) for each variant. If empty, defaults are used.
        /// codex: <workspace>/.agents and $HOME/.agents
        AGENTS_DIRS: &str = "";
        /// anthropic: <workspace>/.claude/agents and $HOME/.claude/agents
        ANTHROPIC_AGENTS_DIRS: &str = "";
        /// vscode: <workspace>/.github/chatmodes and $HOME/.chatmodes
        VSCODE_CHATMODES_DIRS: &str = "";
        /// Tool prefixes per variant
        AGENTS_PREFIX_CODEX: &str = "agent_";
        AGENTS_PREFIX_ANTHROPIC: &str = "anth_";
        AGENTS_PREFIX_VSCODE: &str = "vsc_";
        /// Optional filter for which agents to expose. Comma/whitespace separated.
        AGENTS_FILTER: &str = "";
        /// MCP discovery (stdio only) across providers (Claude/VSCode/Cursor)
        AGENTS_MCP_DISCOVERY: bool = true;
        /// VSCode user mcp.json path (optional)
        VSCODE_USER_MCP: &str = "";
        /// If true, only include MCP servers explicitly referenced by agent tools.
        AGENTS_MCP_LIMIT_REFERENCED: bool = true;
        /// Enable stdio MCP enumeration to gate injection by available tools
        AGENTS_MCP_ENUMERATE: bool = true;
        /// Enumeration timeout per phase in milliseconds
        AGENTS_MCP_ENUM_TIMEOUT_MS: u64 = 4000;
        /// Max servers to enumerate
        AGENTS_MCP_MAX_SERVERS: usize = 128;
        /// Strict gating: drop servers on errors/timeouts
        AGENTS_MCP_ENUM_STRICT: bool = false;
        /// Ambiguous bare refs fallback policy: "none" or "all"
        AGENTS_MCP_ENUM_FALLBACK: &str = "none";
        /// Enable provider tool mapping
        AGENTS_TOOLMAP_ENABLE: bool = true;
        /// Allow mapping to inject custom stdio servers
        AGENTS_TOOLMAP_ALLOW_CUSTOM_SERVERS: bool = true;
        /// Enable model mapping (normalize model/provider tokens)
        AGENTS_MODEL_MAP_ENABLE: bool = true;
        /// Optional model map TOML path; if empty, defaults to <workspace>/.agents/model-map.toml
        AGENTS_MODEL_MAP_FILE: &str = "";
        /// Strict mode for model mapping
        AGENTS_MODEL_MAP_STRICT: bool = false;
        /// Allow model mapping to override user-provided provider
        AGENTS_MODEL_MAP_OVERRIDE_PROVIDER: bool = false;
        /// Normalize provider aliases (e.g., Claude -> anthropic)
        AGENTS_MODEL_MAP_NORMALIZE_PROVIDER: bool = true;
    }

    tracing::info!("starting switchboard-mcp (transport={})", *TRANSPORT);

    // Determine workspace directory (empty -> current execution directory)
    let workspace_dir = if !(*WORKSPACE_DIR).is_empty() {
        std::path::PathBuf::from((*WORKSPACE_DIR).to_string())
    } else {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    };
    tracing::info!("workspace_dir={}", workspace_dir.display());

    // Determine Switchboard home (used for agents and logs)
    let sb_home = if let Ok(sb) = std::env::var("SWITCHBOARD_HOME") {
        if !sb.is_empty() {
            std::path::PathBuf::from(sb)
        } else {
            std::path::PathBuf::new()
        }
    } else {
        std::path::PathBuf::new()
    };
    let sb_home = if sb_home.as_os_str().is_empty() {
        if let Ok(home) = std::env::var("HOME") {
            std::path::PathBuf::from(home).join(".switchboard")
        } else {
            workspace_dir.join(".switchboard")
        }
    } else {
        sb_home
    };
    tracing::info!("switchboard_home={}", sb_home.display());

    // Resolve default directories
    let expand = |p: &str| -> std::path::PathBuf {
        if p.starts_with("~/")
            && let Ok(home) = std::env::var("HOME")
        {
            return std::path::PathBuf::from(home).join(p.trim_start_matches("~/"));
        }
        std::path::PathBuf::from(p)
    };

    let codex_dirs: Vec<_> = if !(*AGENTS_DIRS).is_empty() {
        (*AGENTS_DIRS)
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| expand(s.trim()))
            .collect()
    } else {
        vec![
            workspace_dir.join(".agents"),
            sb_home.join("agents"),
            expand("~/.agents"),
        ]
    };
    tracing::debug!(
        "codex agent dirs: {}",
        codex_dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let anthropic_dirs: Vec<_> = if !(*ANTHROPIC_AGENTS_DIRS).is_empty() {
        (*ANTHROPIC_AGENTS_DIRS)
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| expand(s.trim()))
            .collect()
    } else {
        vec![
            workspace_dir.join(".claude/agents"),
            sb_home.join("agents"),
            expand("~/.claude/agents"),
        ]
    };
    tracing::debug!(
        "anthropic agent dirs: {}",
        anthropic_dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let vscode_dirs: Vec<_> = if !(*VSCODE_CHATMODES_DIRS).is_empty() {
        (*VSCODE_CHATMODES_DIRS)
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| expand(s.trim()))
            .collect()
    } else {
        vec![
            workspace_dir.join(".github/chatmodes"),
            sb_home.join("chatmodes"),
            expand("~/.chatmodes"),
        ]
    };
    tracing::debug!(
        "vscode chatmode dirs: {}",
        vscode_dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Load user config for agent defaults and merge where env not set
    let user_cfg = crate::config::load_user_config(&sb_home).ok().flatten();
    let env_set = |k: &str| std::env::var_os(k).is_some();

    let enable_codex = if env_set("AGENTS_ENABLE_CODEX") {
        *AGENTS_ENABLE_CODEX
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.enable_codex)
            .unwrap_or(*AGENTS_ENABLE_CODEX)
    };
    let enable_anthropic = if env_set("AGENTS_ENABLE_ANTHROPIC") {
        *AGENTS_ENABLE_ANTHROPIC
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.enable_anthropic)
            .unwrap_or(*AGENTS_ENABLE_ANTHROPIC)
    };
    let enable_vscode = if env_set("AGENTS_ENABLE_VSCODE") {
        *AGENTS_ENABLE_VSCODE
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.enable_vscode)
            .unwrap_or(*AGENTS_ENABLE_VSCODE)
    };

    // Allow user config to add extra dirs (appended) when env isn’t explicitly set
    let mut codex_dirs = codex_dirs;
    if !env_set("AGENTS_DIRS")
        && let Some(extra) = user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.codex_dirs.as_ref())
    {
        for p in extra {
            let pb = crate::config::expand_home(p);
            if !codex_dirs.contains(&pb) {
                codex_dirs.push(pb);
            }
        }
    }
    let mut anthropic_dirs = anthropic_dirs;
    if !env_set("ANTHROPIC_AGENTS_DIRS")
        && let Some(extra) = user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.anthropic_dirs.as_ref())
    {
        for p in extra {
            let pb = crate::config::expand_home(p);
            if !anthropic_dirs.contains(&pb) {
                anthropic_dirs.push(pb);
            }
        }
    }
    let mut vscode_dirs = vscode_dirs;
    if !env_set("VSCODE_CHATMODES_DIRS")
        && let Some(extra) = user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.vscode_dirs.as_ref())
    {
        for p in extra {
            let pb = crate::config::expand_home(p);
            if !vscode_dirs.contains(&pb) {
                vscode_dirs.push(pb);
            }
        }
    }

    // Prefixes and other knobs (env wins, else config, else defaults)
    let prefix_codex = if env_set("AGENTS_PREFIX_CODEX") {
        (*AGENTS_PREFIX_CODEX).to_string()
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.prefix_codex.clone())
            .unwrap_or_else(|| (*AGENTS_PREFIX_CODEX).to_string())
    };
    let prefix_anthropic = if env_set("AGENTS_PREFIX_ANTHROPIC") {
        (*AGENTS_PREFIX_ANTHROPIC).to_string()
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.prefix_anthropic.clone())
            .unwrap_or_else(|| (*AGENTS_PREFIX_ANTHROPIC).to_string())
    };
    let prefix_vscode = if env_set("AGENTS_PREFIX_VSCODE") {
        (*AGENTS_PREFIX_VSCODE).to_string()
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.prefix_vscode.clone())
            .unwrap_or_else(|| (*AGENTS_PREFIX_VSCODE).to_string())
    };

    let filter_arg: Option<String> = if !(*AGENTS_FILTER).is_empty() || env_set("AGENTS_FILTER") {
        Some((*AGENTS_FILTER).to_string())
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.filter.clone())
    };

    let enable_mcp_discovery = if env_set("AGENTS_MCP_DISCOVERY") {
        *AGENTS_MCP_DISCOVERY
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.mcp_discovery)
            .unwrap_or(*AGENTS_MCP_DISCOVERY)
    };
    let vscode_user_mcp = if env_set("VSCODE_USER_MCP") && !(*VSCODE_USER_MCP).is_empty() {
        Some(std::path::PathBuf::from((*VSCODE_USER_MCP).to_string()))
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.vscode_user_mcp.as_ref())
            .map(|s| crate::config::expand_home(s))
    };
    let limit_mcp_to_referenced = if env_set("AGENTS_MCP_LIMIT_REFERENCED") {
        *AGENTS_MCP_LIMIT_REFERENCED
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.limit_mcp_to_referenced)
            .unwrap_or(*AGENTS_MCP_LIMIT_REFERENCED)
    };
    let enumerate = if env_set("AGENTS_MCP_ENUMERATE") {
        *AGENTS_MCP_ENUMERATE
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.enumerate)
            .unwrap_or(*AGENTS_MCP_ENUMERATE)
    };
    let enum_timeout_ms = if env_set("AGENTS_MCP_ENUM_TIMEOUT_MS") {
        *AGENTS_MCP_ENUM_TIMEOUT_MS
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.enum_timeout_ms)
            .unwrap_or(*AGENTS_MCP_ENUM_TIMEOUT_MS)
    };
    let enum_max_servers = if env_set("AGENTS_MCP_MAX_SERVERS") {
        *AGENTS_MCP_MAX_SERVERS
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.enum_max_servers)
            .unwrap_or(*AGENTS_MCP_MAX_SERVERS)
    };
    let enum_strict = if env_set("AGENTS_MCP_ENUM_STRICT") {
        *AGENTS_MCP_ENUM_STRICT
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.enum_strict)
            .unwrap_or(*AGENTS_MCP_ENUM_STRICT)
    };
    let enum_fallback_all = if env_set("AGENTS_MCP_ENUM_FALLBACK") {
        (*AGENTS_MCP_ENUM_FALLBACK).eq_ignore_ascii_case("all")
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.enum_fallback_all)
            .unwrap_or_else(|| (*AGENTS_MCP_ENUM_FALLBACK).eq_ignore_ascii_case("all"))
    };
    let toolmap_enable = if env_set("AGENTS_TOOLMAP_ENABLE") {
        *AGENTS_TOOLMAP_ENABLE
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.toolmap_enable)
            .unwrap_or(*AGENTS_TOOLMAP_ENABLE)
    };
    let toolmap_allow_custom_servers = if env_set("AGENTS_TOOLMAP_ALLOW_CUSTOM_SERVERS") {
        *AGENTS_TOOLMAP_ALLOW_CUSTOM_SERVERS
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.toolmap_allow_custom_servers)
            .unwrap_or(*AGENTS_TOOLMAP_ALLOW_CUSTOM_SERVERS)
    };
    let model_map_enable = if env_set("AGENTS_MODEL_MAP_ENABLE") {
        *AGENTS_MODEL_MAP_ENABLE
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.model_map_enable)
            .unwrap_or(*AGENTS_MODEL_MAP_ENABLE)
    };
    let model_map_file = if env_set("AGENTS_MODEL_MAP_FILE") && !(*AGENTS_MODEL_MAP_FILE).is_empty()
    {
        Some(std::path::PathBuf::from(
            (*AGENTS_MODEL_MAP_FILE).to_string(),
        ))
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.model_map_file.as_ref())
            .map(|s| crate::config::expand_home(s))
    };
    let model_map_strict = if env_set("AGENTS_MODEL_MAP_STRICT") {
        *AGENTS_MODEL_MAP_STRICT
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.model_map_strict)
            .unwrap_or(*AGENTS_MODEL_MAP_STRICT)
    };
    let model_map_override_provider = if env_set("AGENTS_MODEL_MAP_OVERRIDE_PROVIDER") {
        *AGENTS_MODEL_MAP_OVERRIDE_PROVIDER
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.model_map_override_provider)
            .unwrap_or(*AGENTS_MODEL_MAP_OVERRIDE_PROVIDER)
    };
    let model_map_normalize_provider = if env_set("AGENTS_MODEL_MAP_NORMALIZE_PROVIDER") {
        *AGENTS_MODEL_MAP_NORMALIZE_PROVIDER
    } else {
        user_cfg
            .as_ref()
            .and_then(|c| c.agents.as_ref())
            .and_then(|a| a.model_map_normalize_provider)
            .unwrap_or(*AGENTS_MODEL_MAP_NORMALIZE_PROVIDER)
    };

    let settings = default_settings(
        workspace_dir.clone(),
        enable_codex,
        enable_anthropic,
        enable_vscode,
        codex_dirs,
        anthropic_dirs,
        vscode_dirs,
        prefix_codex,
        prefix_anthropic,
        prefix_vscode,
        filter_arg,
        enable_mcp_discovery,
        vscode_user_mcp,
        limit_mcp_to_referenced,
        enumerate,
        enum_timeout_ms,
        enum_max_servers,
        enum_strict,
        enum_fallback_all,
        toolmap_enable,
        toolmap_allow_custom_servers,
        model_map_enable,
        model_map_file,
        model_map_strict,
        model_map_override_provider,
        model_map_normalize_provider,
    );

    let agents = match prepare_all(&settings).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("failed to load agents: {} (continuing with empty list)", e);
            Vec::new()
        }
    };
    tracing::info!("loaded {} agent(s) across enabled variants", agents.len());

    // MCP initialize details and capabilities
    let server_details = InitializeResult {
        server_info: Implementation {
            name: "switchboard-mcp".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            title: Some("Switchboard MCP Server".to_string()),
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            ..Default::default()
        },
        meta: None,
        instructions: Some(
            "Call agent_* tools with { task, cwd } (cwd must be an absolute path).".to_string(),
        ),
        protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
    };

    // Handler with in-memory registry
    let handler = AgentsServerHandler::new(agents);

    if *TRANSPORT == "stdio" {
        let transport = StdioTransport::new(TransportOptions::default())?;
        let server: ServerRuntime =
            server_runtime_core::create_server(server_details, transport, handler);
        tracing::info!("starting stdio server");
        if let Err(e) = server.start().await {
            let msg = match e.rpc_error_message() {
                Some(m) => m.to_string(),
                None => e.to_string(),
            };
            tracing::error!("server runtime error: {}", msg);
        }
    } else {
        let host = (*HOST).to_string();
        let port = *PORT;
        let ping = Duration::from_secs(*PING_SECS);
        let server = hyper_server_core::create_server(
            server_details,
            handler,
            HyperServerOptions {
                host: host.clone(),
                port,
                ping_interval: ping,
                enable_json_response: Some(*HTTP_JSON),
                ..Default::default()
            },
        );
        tracing::info!(
            "http server configured; starting listener on {}:{} (json={}, ping_secs={})",
            host,
            port,
            *HTTP_JSON,
            *PING_SECS
        );
        if let Err(e) = server.start().await {
            let msg = match e.rpc_error_message() {
                Some(m) => m.to_string(),
                None => e.to_string(),
            };
            tracing::error!("hyper server error: {}", msg);
        }
    }
    tracing::info!("server stopped");
    Ok(())
}
