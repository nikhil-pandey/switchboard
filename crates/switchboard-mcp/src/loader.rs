//! Agent loader and MCP server discovery/attachment.
//!
//! Responsibilities:
//! - Load agent definitions from supported providers (Codex, Anthropic, VSCode).
//! - Normalize and de-duplicate tool names.
//! - Discover stdio MCP servers (Claude/VSCode/Cursor) and optionally enumerate
//!   tool availability to gate injection.
//! - Apply provider-specific tool mappings and convert embedded server configs
//!   to Codex-compatible structures.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;

use crate::mcp::enumerator::enumerate_stdio;
use crate::mcp::{DiscoveredServers, NormalizedMcpServer, discover_stdio_servers};
use crate::model::{
    AgentConfig, AgentSource, AgentVariant, EnvPrefixes, PreparedAgent, ResolvedAgent, safe_name,
    tool_name_for, tool_prefix_for,
};
use crate::modelmap::{ApplyOptions as ModelApplyOptions, ModelMap};
use crate::parser::AgentParser;
use crate::parser::anthropic_frontmatter::AnthropicFrontmatterParser;
use crate::parser::codex_toml::CodexTomlParser;
use crate::parser::vscode_chatmode::VscodeChatmodeParser;
use crate::toolmap::types::ProviderId;
use crate::toolmap::{apply_mapping, default_mapping};

/// Loader configuration controlling which providers/paths to scan and how to
/// resolve and attach MCP servers.
#[derive(Debug, Clone)]
pub struct LoaderSettings {
    /// Workspace directory used for relative discovery (e.g., `.mcp.json`).
    pub workspace_dir: PathBuf,
    /// Enable Codex agents and scan `codex_dirs`.
    pub enable_codex: bool,
    /// Enable Anthropic agents and scan `anthropic_dirs`.
    pub enable_anthropic: bool,
    /// Enable VSCode chatmode agents and scan `vscode_dirs`.
    pub enable_vscode: bool,
    pub codex_dirs: Vec<PathBuf>,
    pub anthropic_dirs: Vec<PathBuf>,
    pub vscode_dirs: Vec<PathBuf>,
    /// Tool name prefixes per provider (e.g., `agent_`).
    pub prefix_codex: String,
    pub prefix_anthropic: String,
    pub prefix_vscode: String,
    /// Optional filter by agent name/safe_name/tag.
    pub filter: Option<String>,
    /// Discover stdio MCP servers from user/project config files.
    pub enable_mcp_discovery: bool,
    /// Optional VSCode user `mcp.json` path.
    pub vscode_user_mcp_path: Option<PathBuf>,
    /// If true, only include MCP servers explicitly referenced by agent tools.
    pub limit_mcp_to_referenced: bool,
    /// Enumerate candidate MCP servers and gate injection by tool availability.
    pub enumerate: bool,
    /// Enumeration timeout per server.
    pub enum_timeout_ms: u64,
    /// Cap on number of servers to enumerate.
    pub enum_max_servers: usize,
    /// Strict mode: drop servers on enumeration errors or timeouts.
    pub enum_strict: bool,
    /// When resolving bare tool refs, include all matches instead of none on ambiguity.
    pub enum_fallback_all: bool,
    /// Enable provider tool mapping (aliases → builtins, custom servers, etc.).
    pub toolmap_enable: bool,
    /// Allow mapping to inject custom stdio servers.
    pub toolmap_allow_custom_servers: bool,
    /// Enable model mapping (normalize model/provider tokens to canonical IDs).
    pub model_map_enable: bool,
    /// Optional mapping file path; if None, defaults to `<workspace>/.agents/model-map.toml`.
    pub model_map_file: Option<PathBuf>,
    /// Strict mode for model mapping: warn on unknown/ambiguous and keep as-is.
    pub model_map_strict: bool,
    /// Allow model mapping to override user-provided provider.
    pub model_map_override_provider: bool,
    /// Normalize provider aliases (e.g., "Claude" -> "anthropic").
    pub model_map_normalize_provider: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn default_settings(
    workspace_dir: PathBuf,
    enable_codex: bool,
    enable_anthropic: bool,
    enable_vscode: bool,
    codex_dirs: Vec<PathBuf>,
    anthropic_dirs: Vec<PathBuf>,
    vscode_dirs: Vec<PathBuf>,
    prefix_codex: String,
    prefix_anthropic: String,
    prefix_vscode: String,
    filter: Option<String>,
    enable_mcp_discovery: bool,
    vscode_user_mcp_path: Option<PathBuf>,
    limit_mcp_to_referenced: bool,
    enumerate: bool,
    enum_timeout_ms: u64,
    enum_max_servers: usize,
    enum_strict: bool,
    enum_fallback_all: bool,
    toolmap_enable: bool,
    toolmap_allow_custom_servers: bool,
    model_map_enable: bool,
    model_map_file: Option<PathBuf>,
    model_map_strict: bool,
    model_map_override_provider: bool,
    model_map_normalize_provider: bool,
) -> LoaderSettings {
    LoaderSettings {
        workspace_dir,
        enable_codex,
        enable_anthropic,
        enable_vscode,
        codex_dirs,
        anthropic_dirs,
        vscode_dirs,
        prefix_codex,
        prefix_anthropic,
        prefix_vscode,
        filter,
        enable_mcp_discovery,
        vscode_user_mcp_path,
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
    }
}

/// Load, normalize, and prepare agents across enabled providers.
pub async fn prepare_all(settings: &LoaderSettings) -> anyhow::Result<Vec<PreparedAgent>> {
    let mut agents: Vec<ResolvedAgent> = Vec::new();
    let env_prefixes = EnvPrefixes {
        codex: settings.prefix_codex.as_str(),
        anthropic: settings.prefix_anthropic.as_str(),
        vscode: settings.prefix_vscode.as_str(),
    };

    if settings.enable_codex {
        agents.extend(load_variant(
            AgentVariant::Codex,
            &settings.codex_dirs,
            &env_prefixes,
        )?);
    }
    if settings.enable_anthropic {
        agents.extend(load_variant(
            AgentVariant::Anthropic,
            &settings.anthropic_dirs,
            &env_prefixes,
        )?);
    }
    if settings.enable_vscode {
        agents.extend(load_variant(
            AgentVariant::Vscode,
            &settings.vscode_dirs,
            &env_prefixes,
        )?);
    }

    dedupe_tool_names(&mut agents);

    // Discover and attach MCP servers (stdio only)
    let mut discovered = if settings.enable_mcp_discovery {
        discover_stdio_servers(
            &settings.workspace_dir,
            settings.vscode_user_mcp_path.as_deref(),
        )
    } else {
        DiscoveredServers::default()
    };
    // Avoid recursive self-attachment: skip any discovered server that looks like
    // this Switchboard MCP itself (e.g., command name "switchboard-mcp" or key "switchboard").
    // Can be disabled by setting SWITCHBOARD_SKIP_SELF=false.
    let skip_self = std::env::var("SWITCHBOARD_SKIP_SELF")
        .map(|v| !(v.eq_ignore_ascii_case("false") || v == "0"))
        .unwrap_or(true);
    if skip_self && !discovered.by_key.is_empty() {
        let before = discovered.by_key.len();
        discovered.by_key.retain(|k, srv| match &srv.transport {
            crate::mcp::types::McpTransport::Stdio { command, .. } => {
                let file = Path::new(command)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let looks_like_self =
                    k.eq_ignore_ascii_case("switchboard") || file.starts_with("switchboard-mcp");
                if looks_like_self {
                    tracing::info!(
                        "skipping self 'switchboard' MCP server '{}' (command={})",
                        k,
                        command
                    );
                    false
                } else {
                    true
                }
            }
        });
        let after = discovered.by_key.len();
        if after < before {
            tracing::info!(
                "filtered out self switchboard servers: {} → {}",
                before,
                after
            );
        }
    }
    if !discovered.by_key.is_empty() {
        for (k, srv) in &discovered.by_key {
            let note = srv.origin.note.as_deref().unwrap_or("");
            let path = srv
                .origin
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unknown>".to_string());
            tracing::info!(
                "discovered server '{}' via {:?} @ {} {}",
                k,
                srv.origin.provider,
                path,
                note
            );
        }
    }

    // Apply provider tool mapping first (transform bare refs to toggles or
    // namespaced refs and optionally inject custom servers)
    if settings.toolmap_enable {
        let mapping = default_mapping();
        for ra in agents.iter_mut() {
            let provider = match ra.source.variant {
                AgentVariant::Codex => ProviderId::Codex,
                AgentVariant::Anthropic => ProviderId::Anthropic,
                AgentVariant::Vscode => ProviderId::Vscode,
            };
            let mut cfg = ra.config.clone();
            let customs = apply_mapping(
                provider,
                &mut cfg,
                &mapping,
                settings.toolmap_allow_custom_servers,
            );
            // Merge mapping-produced servers into discovered set (if not present)
            for srv in customs {
                discovered.by_key.entry(srv.key.clone()).or_insert(srv);
            }
            ra.config = cfg;
        }
    }

    // Load model mapping if enabled
    let model_map: Option<ModelMap> = if settings.model_map_enable {
        let default_path = settings
            .workspace_dir
            .join(".agents")
            .join("model-map.toml");
        let path = settings.model_map_file.as_ref().unwrap_or(&default_path);
        match crate::modelmap::load_from_file(path) {
            Ok(m) => {
                tracing::info!(
                    "loaded model map from {} (tokens={}, provider_aliases={})",
                    path.display(),
                    m.by_token.len(),
                    m.provider_aliases.len()
                );
                Some(m)
            }
            Err(e) => {
                if settings.model_map_file.is_some() {
                    tracing::warn!("failed to load model map {}: {}", path.display(), e);
                } else {
                    tracing::debug!("no default model map at {}: {}", path.display(), e);
                }
                Some(crate::modelmap::load_default())
            }
        }
    } else {
        None
    };
    if !discovered.by_key.is_empty() {
        // Optionally enumerate and gate servers by tool availability
        if settings.enumerate {
            let timeout = std::time::Duration::from_millis(settings.enum_timeout_ms);
            // Build candidate server set
            use std::collections::HashSet as Set;
            let mut candidates: Set<String> = Set::new();
            for ra in &agents {
                if let Some(refs) = &ra.config.mcp_tool_refs {
                    let has_bare = refs
                        .iter()
                        .any(|r| matches!(r, crate::mcp::types::McpToolRef::Bare { .. }));
                    // Namespaced refs: include exact servers
                    for r in refs {
                        if let crate::mcp::types::McpToolRef::Namespaced { server_key, .. } = r {
                            candidates.insert(server_key.clone());
                        }
                    }
                    // Bare refs: include all discovered for now
                    if has_bare {
                        for k in discovered.by_key.keys() {
                            candidates.insert(k.clone());
                        }
                    }
                }
            }
            // Log enumeration plan
            tracing::info!(
                "enumerating MCP servers: candidates={}, timeout_ms={}, max_servers={}",
                candidates.len(),
                settings.enum_timeout_ms,
                settings.enum_max_servers
            );
            tracing::debug!("enumeration candidates: {:?}", candidates);
            // Enumerate up to the cap
            let mut inventory: std::collections::HashMap<
                String,
                std::collections::HashSet<String>,
            > = std::collections::HashMap::new();
            let keys: Vec<String> = candidates
                .into_iter()
                .take(settings.enum_max_servers)
                .collect();
            let mut set = tokio::task::JoinSet::new();
            for key in keys {
                if let Some(srv) = discovered.by_key.get(&key) {
                    let srv = srv.clone();
                    set.spawn(
                        async move { (srv.key.clone(), enumerate_stdio(&srv, timeout).await) },
                    );
                }
            }
            while let Some(res) = set.join_next().await {
                match res {
                    Ok((_key, Ok(st))) => {
                        inventory.insert(st.key, st.tools);
                    }
                    Ok((k, Err(e))) => {
                        if settings.enum_strict {
                            // In strict mode, drop from discovered by not adding inventory entry.
                        } else {
                            tracing::warn!("enumeration failed for server {}: {}", k, e);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("enumeration task join error: {}", e);
                    }
                }
            }
            // Log enumerated tools per server (summarized)
            for (srv, tools) in &inventory {
                let summary = summarize_tools(tools, 10);
                tracing::info!("enumerated server '{}' tools: {}", srv, summary);
            }
            // Remove servers that failed to enumerate from consideration
            let before = discovered.by_key.len();
            discovered.by_key.retain(|k, _| inventory.contains_key(k));
            let after = discovered.by_key.len();
            if after < before {
                tracing::info!(
                    "filtered MCP servers to enumerated set: {} → {}",
                    before,
                    after
                );
            }
            // Gate injection by inventory
            for ra in agents.iter_mut() {
                gate_by_inventory(
                    ra,
                    &discovered,
                    &inventory,
                    settings.enum_fallback_all,
                    settings.limit_mcp_to_referenced,
                );
            }
        } else {
            for ra in agents.iter_mut() {
                attach_mcp_servers_to_agent(ra, &discovered, settings.limit_mcp_to_referenced);
            }
        }
    }

    // Apply filter (by name, safe_name, or tags)
    if let Some(filter) = settings.filter.as_ref().filter(|s| !s.trim().is_empty()) {
        let allowed = build_allowed_set(filter);
        agents.retain(|ra| match_agent(&ra.config, &allowed));
    }
    // Build PreparedAgent list
    let mut prepared: Vec<PreparedAgent> = Vec::with_capacity(agents.len());
    for ra in &agents {
        let instructions = read_instructions(&ra.config);
        let servers_cfg = convert_servers_for_agent(ra);
        // Optionally apply model mapping to the agent config before preparing
        let mut cfg = ra.config.clone();
        if let Some(mm) = &model_map {
            crate::modelmap::apply_to_agent(
                &mut cfg,
                mm,
                ModelApplyOptions {
                    normalize_provider: settings.model_map_normalize_provider,
                    override_provider: settings.model_map_override_provider,
                    strict: settings.model_map_strict,
                },
            );
        }
        prepared.push(PreparedAgent {
            tool_name: ra.tool_name.clone(),
            name: cfg.name.clone(),
            description: cfg.description.clone(),
            tags: cfg.tags.clone(),
            provider: ra.source.variant,
            mcp_servers: servers_cfg,
            instructions,
            run: cfg.run.clone(),
        });
    }

    // Final per-agent summary
    for ra in &agents {
        let server_keys: Vec<String> = ra.mcp_servers.keys().cloned().collect();
        let ns_total = ra
            .config
            .mcp_tool_refs
            .as_ref()
            .map(|v| {
                v.iter()
                    .filter(|r| matches!(r, crate::mcp::types::McpToolRef::Namespaced { .. }))
                    .count()
            })
            .unwrap_or(0);
        let ns_matched = ra
            .config
            .mcp_tool_refs
            .as_ref()
            .map(|v| {
                v.iter()
                    .filter_map(|r| match r {
                        crate::mcp::types::McpToolRef::Namespaced { server_key, .. } => {
                            Some(server_key)
                        }
                        _ => None,
                    })
                    .filter(|k| ra.mcp_servers.contains_key(*k))
                    .count()
            })
            .unwrap_or(0);
        let bare_total = ra
            .config
            .mcp_tool_refs
            .as_ref()
            .map(|v| {
                v.iter()
                    .filter(|r| matches!(r, crate::mcp::types::McpToolRef::Bare { .. }))
                    .count()
            })
            .unwrap_or(0);
        tracing::info!(
            "agent '{}' (tool={} from {:?}): servers=[{}]; namespaced matched {}/{}; bare {}",
            ra.config.name,
            ra.tool_name,
            ra.source.variant,
            server_keys.join(", "),
            ns_matched,
            ns_total,
            bare_total,
        );
    }

    Ok(prepared)
}

fn load_variant(
    variant: AgentVariant,
    dirs: &[PathBuf],
    env_prefixes: &EnvPrefixes,
) -> anyhow::Result<Vec<ResolvedAgent>> {
    let prefix = tool_prefix_for(variant, env_prefixes).to_string();
    let parser_kind = match variant {
        AgentVariant::Codex => 0,
        AgentVariant::Anthropic => 1,
        AgentVariant::Vscode => 2,
    };
    let mut out = Vec::new();
    for dir in dirs.iter() {
        if !dir.exists() || !dir.is_dir() {
            continue;
        }
        tracing::debug!("scanning {:?} dir {}", variant, dir.display());
        for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let content = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("skipping {}: failed to read: {}", path.display(), e);
                    continue;
                }
            };
            let parsed: Option<AgentConfig> = match parser_kind {
                0 if CodexTomlParser::supports(&path) => {
                    Some(match CodexTomlParser::parse(&content, &path) {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            tracing::warn!("skipping {}: {:#}", path.display(), e);
                            continue;
                        }
                    })
                }
                1 if AnthropicFrontmatterParser::supports(&path) => {
                    Some(match AnthropicFrontmatterParser::parse(&content, &path) {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            tracing::warn!("skipping {}: {:#}", path.display(), e);
                            continue;
                        }
                    })
                }
                2 if VscodeChatmodeParser::supports(&path) => {
                    Some(match VscodeChatmodeParser::parse(&content, &path) {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            tracing::warn!("skipping {}: {:#}", path.display(), e);
                            continue;
                        }
                    })
                }
                _ => None,
            };
            let Some(mut cfg) = parsed else { continue };

            // Provide default description if empty
            if cfg.description.trim().is_empty() {
                cfg.description = format!("Agent '{}': Execute tasks via Codex", cfg.name);
            }

            // Map codex toggles (if provided) into run policy
            if matches!(variant, AgentVariant::Codex)
                && let Some(toggles) = cfg.toggles.take()
            {
                let mut run = cfg.run.take().unwrap_or_default();
                if toggles.include_plan_tool.is_some() {
                    run.include_plan_tool = toggles.include_plan_tool;
                }
                if toggles.include_apply_patch_tool.is_some() {
                    run.include_apply_patch_tool = toggles.include_apply_patch_tool;
                }
                if toggles.include_view_image_tool.is_some() {
                    run.include_view_image_tool = toggles.include_view_image_tool;
                }
                if toggles.tools_web_search_request.is_some() {
                    run.tools_web_search_request = toggles.tools_web_search_request;
                }
                cfg.run = Some(run);
            }

            let tool_name = tool_name_for(&prefix, &cfg.name);
            out.push(ResolvedAgent {
                source: AgentSource {
                    variant,
                    path: path.clone(),
                },
                config: cfg,
                tool_name,
                mcp_servers: HashMap::new(),
            });
        }
    }
    Ok(out)
}

fn summarize_tools(set: &HashSet<String>, max_show: usize) -> String {
    if set.is_empty() {
        return "<none>".to_string();
    }
    let mut v: Vec<&String> = set.iter().collect();
    v.sort();
    let shown: Vec<String> = v.iter().take(max_show).map(|s| (*s).clone()).collect();
    if set.len() > max_show {
        format!("{} (+{} more)", shown.join(", "), set.len() - max_show)
    } else {
        shown.join(", ")
    }
}

pub fn dedupe_tool_names(records: &mut [ResolvedAgent]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for rec in records.iter_mut() {
        let mut candidate = rec.tool_name.clone();
        if let Some(count) = seen.get_mut(&candidate) {
            *count += 1;
            candidate = format!("{}_{}", candidate, *count);
        } else {
            seen.insert(candidate.clone(), 1);
        }
        rec.tool_name = candidate;
    }
}

fn build_allowed_set(filter: &str) -> HashSet<String> {
    let mut allowed: HashSet<String> = HashSet::new();
    for tok in filter
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.trim().is_empty())
    {
        allowed.insert(tok.trim().to_ascii_lowercase());
    }
    allowed
}

fn match_agent(cfg: &AgentConfig, allowed: &HashSet<String>) -> bool {
    let name = cfg.name.to_ascii_lowercase();
    let safe = safe_name(&cfg.name);
    if allowed.contains(&name) || allowed.contains(&safe) {
        return true;
    }
    if let Some(tags) = &cfg.tags {
        for t in tags {
            if allowed.contains(&t.to_ascii_lowercase()) {
                return true;
            }
        }
    }
    false
}

fn read_instructions(agent: &AgentConfig) -> Option<String> {
    if let Some(instr_path) = agent.instructions_file.as_ref()
        && let Ok(s) = std::fs::read_to_string(instr_path)
        && !s.trim().is_empty()
    {
        return Some(s);
    }
    agent.instructions.clone()
}

fn to_mcp_server_config(
    srv: &crate::mcp::types::NormalizedMcpServer,
) -> Option<codex_core::config_types::McpServerConfig> {
    match &srv.transport {
        crate::mcp::types::McpTransport::Stdio { command, args, env } => {
            let mut tbl = toml::Table::new();
            tbl.insert("command".to_string(), toml::Value::String(command.clone()));
            if !args.is_empty() {
                tbl.insert(
                    "args".to_string(),
                    toml::Value::Array(
                        args.iter()
                            .map(|s| toml::Value::String(s.clone()))
                            .collect(),
                    ),
                );
            }
            if !env.is_empty() {
                let mut env_tbl = toml::Table::new();
                for (k, v) in env.iter() {
                    env_tbl.insert(k.clone(), toml::Value::String(v.clone()));
                }
                tbl.insert("env".to_string(), toml::Value::Table(env_tbl));
            }
            match toml::Value::Table(tbl).try_into() {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    tracing::warn!("failed to convert stdio mcp server '{}': {}", srv.key, e);
                    None
                }
            }
        }
    }
}

fn convert_servers_for_agent(
    ra: &ResolvedAgent,
) -> std::collections::HashMap<String, codex_core::config_types::McpServerConfig> {
    use codex_core::config_types::McpServerConfig;
    use std::collections::HashMap;
    let mut out: HashMap<String, McpServerConfig> = HashMap::new();
    // Convert discovered/mapped servers
    for (k, n) in &ra.mcp_servers {
        if let Some(cfg) = to_mcp_server_config(n) {
            out.insert(k.clone(), cfg);
        }
    }
    // Convert embedded codex servers from agent TOML if any
    if let Some(mcp_val) = &ra.config.mcp_servers {
        match mcp_val
            .clone()
            .try_into::<HashMap<String, McpServerConfig>>()
        {
            Ok(map) => {
                for (k, v) in map {
                    out.insert(k, v);
                }
            }
            Err(e) => tracing::warn!(
                "failed to parse embedded mcp_servers for {}: {}",
                ra.config.name,
                e
            ),
        }
    }
    out
}

fn attach_mcp_servers_to_agent(
    ra: &mut ResolvedAgent,
    discovered: &DiscoveredServers,
    limit_to_referenced: bool,
) {
    if discovered.by_key.is_empty() {
        return;
    }
    if !limit_to_referenced {
        ra.mcp_servers = discovered.by_key.clone();
        return;
    }
    // Limit to referenced: include only explicitly referenced servers if any
    if let Some(refs) = ra.config.mcp_tool_refs.as_ref() {
        let mut selected: HashMap<String, NormalizedMcpServer> = HashMap::new();
        let mut has_bare = false;
        for r in refs {
            match r {
                crate::mcp::types::McpToolRef::Namespaced { server_key, .. } => {
                    if let Some(srv) = discovered.by_key.get(server_key) {
                        selected.insert(server_key.clone(), srv.clone());
                    }
                }
                crate::mcp::types::McpToolRef::Bare { .. } => {
                    has_bare = true;
                }
            }
        }
        if has_bare {
            // Bare refs cannot be resolved without enumeration; include all to be safe.
            ra.mcp_servers = discovered.by_key.clone();
        } else {
            ra.mcp_servers = selected;
        }
    }
}

fn gate_by_inventory(
    ra: &mut ResolvedAgent,
    discovered: &DiscoveredServers,
    inventory: &std::collections::HashMap<String, std::collections::HashSet<String>>,
    fallback_all_on_ambiguous: bool,
    limit_to_referenced: bool,
) {
    // Build initial set according to referenced policy
    let mut selected: HashMap<String, NormalizedMcpServer> = HashMap::new();
    if let Some(refs) = ra.config.mcp_tool_refs.as_ref() {
        // For namespaced refs: only keep servers where tool is present in inventory
        let mut missing_namespaced: Vec<String> = Vec::new();
        let mut missing_bare: Vec<String> = Vec::new();
        for r in refs {
            match r {
                crate::mcp::types::McpToolRef::Namespaced { server_key, tool } => {
                    if let Some(tools) = inventory.get(server_key) {
                        if tools.contains(tool) {
                            if let Some(srv) = discovered.by_key.get(server_key) {
                                selected.insert(server_key.clone(), srv.clone());
                            }
                        } else {
                            missing_namespaced.push(format!("{}/{}", server_key, tool));
                        }
                    } else {
                        missing_namespaced.push(format!("{}/{} (no inventory)", server_key, tool));
                    }
                }
                crate::mcp::types::McpToolRef::Bare { tool } => {
                    // Bare refs: find all servers exposing 'tool'
                    let matches: Vec<&String> = inventory
                        .iter()
                        .filter_map(|(k, tools)| if tools.contains(tool) { Some(k) } else { None })
                        .collect();
                    if matches.is_empty() {
                        missing_bare.push(tool.clone());
                    } else if matches.len() == 1 {
                        let k = matches[0];
                        if let Some(srv) = discovered.by_key.get(k) {
                            selected.insert(k.clone(), srv.clone());
                        }
                    } else {
                        tracing::warn!(
                            "ambiguous bare tool '{}' matches multiple servers: {:?}",
                            tool,
                            matches
                        );
                        if fallback_all_on_ambiguous {
                            for k in matches {
                                if let Some(srv) = discovered.by_key.get(k) {
                                    selected.insert(k.clone(), srv.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        if !missing_namespaced.is_empty() {
            tracing::warn!(
                "agent '{}' namespaced tools not found: {}",
                ra.config.name,
                missing_namespaced.join(", ")
            );
        }
        if !missing_bare.is_empty() {
            tracing::warn!(
                "agent '{}' bare tools not found: {}",
                ra.config.name,
                missing_bare.join(", ")
            );
        }
    }
    if selected.is_empty() && !limit_to_referenced {
        // If nothing selected (no refs or filtered out) but not limited, include all
        ra.mcp_servers = discovered.by_key.clone();
    } else {
        ra.mcp_servers = selected;
    }
}
