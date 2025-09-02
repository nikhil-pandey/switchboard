//! Mapping tables from provider-specific tool names to Codex toggles or MCP servers.

use std::collections::HashMap;

/// Provider identifiers for mapping tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderId {
    Codex,
    Anthropic,
    Vscode,
}

/// Built-in toggles provided by Codex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinToggle {
    Plan,
    ApplyPatch,
    ViewImage,
    WebSearch,
    /// Provided by default via Codex terminal access; consume without toggles.
    TerminalAccess,
}

impl BuiltinToggle {}

/// Destination for a mapped tool: either a built-in Codex toggle or a named MCP tool with launch details.
#[derive(Debug, Clone)]
pub enum MappingDest {
    Builtin(BuiltinToggle),
    McpTool {
        server_key: String,
        tool: String,
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
}

/// Mapping table and aliases for a single provider.
#[derive(Debug, Default, Clone)]
pub struct ToolMappingTable {
    pub map: HashMap<String, MappingDest>,
    pub aliases: HashMap<String, String>,
}

/// Loaded mapping set across providers.
#[derive(Debug, Default, Clone)]
pub struct LoadedMapping {
    pub vscode: ToolMappingTable,
    pub anthropic: ToolMappingTable,
}

pub fn default_mapping() -> LoadedMapping {
    // Conservative defaults; easy to extend later.
    let mut vscode = ToolMappingTable::default();
    // VSCode "memory" → server-memory
    vscode.map.insert(
        "memory".to_string(),
        MappingDest::McpTool {
            server_key: "memory".to_string(),
            tool: "memory".to_string(),
            command: "npx".to_string(),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-memory".to_string(),
            ],
            env: HashMap::new(),
        },
    );
    // Closest mappings for VS Code vendor tools → Codex toggles
    // - edit/new → apply_patch (file edits/creation)
    // - search/fetch/githubRepo → web_search (best-effort web capability)
    // - runCommands → TerminalAccess (provided by default terminal; no toggle)
    vscode.map.insert(
        "edit".to_string(),
        MappingDest::Builtin(BuiltinToggle::ApplyPatch),
    );
    vscode.map.insert(
        "new".to_string(),
        MappingDest::Builtin(BuiltinToggle::ApplyPatch),
    );
    for k in ["search", "fetch", "githubRepo"] {
        vscode.map.insert(
            k.to_string(),
            MappingDest::Builtin(BuiltinToggle::WebSearch),
        );
    }
    vscode.map.insert(
        "runCommands".to_string(),
        MappingDest::Builtin(BuiltinToggle::TerminalAccess),
    );

    // Aliases and explicit placeholders for unmapped VS Code tools
    // Keep these enumerated so we know to revisit them later.
    for (alias, canon) in [
        ("Find Usages", "usages"),
        ("runTasks", "runTasks"),
        ("notebooks", "notebooks"),
        ("extensions", "extensions"),
        ("usages", "usages"),
        ("vscodeAPI", "vscodeAPI"),
        ("problems", "problems"),
        ("changes", "changes"),
        ("testFailure", "testFailure"),
        ("openSimpleBrowser", "openSimpleBrowser"),
    ] {
        vscode.aliases.insert(alias.to_string(), canon.to_string());
    }

    let mut anthropic = ToolMappingTable::default();
    for (k, t) in [
        ("plan", BuiltinToggle::Plan),
        ("apply_patch", BuiltinToggle::ApplyPatch),
        ("view_image", BuiltinToggle::ViewImage),
        ("web_search", BuiltinToggle::WebSearch),
    ] {
        anthropic.map.insert(k.to_string(), MappingDest::Builtin(t));
    }

    // Anthropic/Claude tool aliases → closest Codex toggles
    // Editing family → apply_patch
    for k in ["Edit", "MultiEdit", "Write", "NotebookEdit"] {
        anthropic
            .aliases
            .insert(k.to_string(), "apply_patch".to_string());
    }
    // Web capabilities
    anthropic
        .aliases
        .insert("WebSearch".to_string(), "web_search".to_string());
    anthropic
        .aliases
        .insert("WebFetch".to_string(), "web_search".to_string());
    // Planning/todo → plan
    anthropic
        .aliases
        .insert("TodoWrite".to_string(), "plan".to_string());

    // Terminal-related Claude tools are provided by default terminal access → consume
    for terminal_tool in ["Bash", "Glob", "Grep", "Read", "BashOutput", "KillBash"] {
        anthropic.map.insert(
            terminal_tool.to_string(),
            MappingDest::Builtin(BuiltinToggle::TerminalAccess),
        );
    }

    LoadedMapping { vscode, anthropic }
}
