//! Load stdio MCP server definitions from common host config files.
//!
//! Supported sources:
//! - Claude project `.mcp.json` and user `~/.claude.json` (project + global).
//! - VSCode project `.vscode/mcp.json` or explicit user file.
//! - Cursor user `~/.cursor/mcp.json`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;

use super::{DiscoveredServers, McpProvider, McpServerOrigin, McpTransport, NormalizedMcpServer};

fn expand_home(p: &str) -> PathBuf {
    if p.starts_with("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(p.trim_start_matches("~/"));
    }
    PathBuf::from(p)
}

/// Discover stdio-capable MCP servers from known config paths.
pub fn discover_stdio_servers(
    workspace_dir: &Path,
    vscode_user_mcp: Option<&Path>,
) -> DiscoveredServers {
    let mut by_key: HashMap<String, NormalizedMcpServer> = HashMap::new();

    // Claude: project .mcp.json
    let claude_project = workspace_dir.join(".mcp.json");
    merge_claude_project(&claude_project, &mut by_key);

    // Claude: user ~/.claude.json
    let claude_user = expand_home("~/.claude.json");
    merge_claude_user(&claude_user, workspace_dir, &mut by_key);

    // VSCode: project .vscode/mcp.json
    let vscode_project = workspace_dir.join(".vscode/mcp.json");
    merge_vscode_mcp(&vscode_project, &mut by_key);

    // VSCode: user config if provided via env
    if let Some(p) = vscode_user_mcp {
        merge_vscode_mcp(p, &mut by_key);
    }

    // Cursor: user ~/.cursor/mcp.json
    let cursor_user = expand_home("~/.cursor/mcp.json");
    merge_cursor_mcp(&cursor_user, &mut by_key);

    DiscoveredServers { by_key }
}

/// Merge Claude project `.mcp.json` entries.
fn merge_claude_project(path: &Path, out: &mut HashMap<String, NormalizedMcpServer>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<JsonValue>(&content) else {
        return;
    };
    let Some(map) = v.get("mcpServers").and_then(|m| m.as_object()) else {
        return;
    };
    for (key, def) in map.iter() {
        if let Some(mut srv) = parse_stdio_like(key, def) {
            srv.origin = McpServerOrigin {
                provider: McpProvider::Claude,
                path: Some(path.to_path_buf()),
                note: Some(".mcp.json".to_string()),
            };
            out.insert(srv.key.clone(), srv);
        }
    }
}

/// Merge Claude user `~/.claude.json` project-scoped and global entries.
fn merge_claude_user(
    path: &Path,
    workspace_dir: &Path,
    out: &mut HashMap<String, NormalizedMcpServer>,
) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<JsonValue>(&content) else {
        return;
    };

    let mut merged: HashMap<String, NormalizedMcpServer> = HashMap::new();

    // project-scoped servers
    if let Some(projects) = v.get("projects").and_then(|p| p.as_object()) {
        // use absolute workspace path key
        let ws_key = workspace_dir
            .canonicalize()
            .unwrap_or_else(|_| workspace_dir.to_path_buf());
        if let Some(proj) = projects
            .get(ws_key.to_string_lossy().as_ref())
            .and_then(|v| v.as_object())
        {
            if let Some(m) = proj.get("mcpServers").and_then(|m| m.as_object()) {
                for (k, def) in m {
                    if let Some(mut srv) = parse_claude_server(k, def) {
                        srv.origin = McpServerOrigin {
                            provider: McpProvider::Claude,
                            path: Some(path.to_path_buf()),
                            note: Some(format!("~/.claude.json project: {}", ws_key.display())),
                        };
                        merged.insert(srv.key.clone(), srv);
                    }
                }
            }
            // Apply enabled/disabled lists if present
            let enabled: Option<Vec<String>> = proj
                .get("enabledMcpjsonServers")
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                });
            let disabled: Vec<String> = proj
                .get("disabledMcpjsonServers")
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(allow) = enabled {
                merged.retain(|k, _| allow.iter().any(|a| a.eq_ignore_ascii_case(k)));
            }
            if !disabled.is_empty() {
                for k in disabled {
                    merged.remove(&k);
                }
            }
        }
    }

    // user-global servers
    if let Some(m) = v.get("mcpServers").and_then(|m| m.as_object()) {
        for (k, def) in m {
            if let Some(mut srv) = parse_claude_server(k, def) {
                srv.origin = McpServerOrigin {
                    provider: McpProvider::Claude,
                    path: Some(path.to_path_buf()),
                    note: Some("~/.claude.json global".to_string()),
                };
                merged.entry(srv.key.clone()).or_insert(srv);
            }
        }
    }

    out.extend(merged);
}

/// Merge VSCode `mcp.json` entries (project or user-provided path).
///
/// VS Code project format typically uses a top-level `servers` table, while
/// some examples reuse the Claude/Cursor-style `mcpServers`. Be tolerant and
/// accept either key, preferring `servers` when both are present.
fn merge_vscode_mcp(path: &Path, out: &mut HashMap<String, NormalizedMcpServer>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<JsonValue>(&content) else {
        return;
    };
    // Prefer VS Code's `servers` key; fall back to `mcpServers` if present.
    let map = v
        .get("servers")
        .and_then(|m| m.as_object())
        .or_else(|| v.get("mcpServers").and_then(|m| m.as_object()));
    let Some(map) = map else {
        return;
    };
    for (key, def) in map.iter() {
        if let Some(mut srv) = parse_stdio_like(key, def) {
            srv.origin = McpServerOrigin {
                provider: McpProvider::Vscode,
                path: Some(path.to_path_buf()),
                note: Some(".vscode/mcp.json".to_string()),
            };
            // Project/user-specified VS Code servers should take precedence over any
            // previously merged global defaults (e.g., Claude user config).
            out.insert(srv.key.clone(), srv);
        }
    }
}

/// Merge Cursor `~/.cursor/mcp.json` entries.
fn merge_cursor_mcp(path: &Path, out: &mut HashMap<String, NormalizedMcpServer>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<JsonValue>(&content) else {
        return;
    };
    let Some(map) = v.get("mcpServers").and_then(|m| m.as_object()) else {
        return;
    };
    for (key, def) in map.iter() {
        if let Some(mut srv) = parse_claude_server(key, def) {
            srv.origin = McpServerOrigin {
                provider: McpProvider::Cursor,
                path: Some(path.to_path_buf()),
                note: Some("~/.cursor/mcp.json".to_string()),
            };
            out.entry(srv.key.clone()).or_insert(srv);
        }
    }
}

/// Parse stdio-style server entry: `{ command, args?, env? }`.
fn parse_stdio_like(key: &str, def: &JsonValue) -> Option<NormalizedMcpServer> {
    let command = def.get("command").and_then(|v| v.as_str())?.to_string();
    let args: Vec<String> = def
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let env: HashMap<String, String> = def
        .get("env")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Some(NormalizedMcpServer {
        key: key.to_string(),
        transport: McpTransport::Stdio { command, args, env },
        origin: McpServerOrigin {
            provider: McpProvider::Claude,
            path: None,
            note: None,
        },
    })
}

/// Parse Claude server entry, ignoring HTTP or URL-only entries.
///
/// Accepts either `{ command, args, env }` or `{ type: "stdio", ... }`.
fn parse_claude_server(key: &str, def: &JsonValue) -> Option<NormalizedMcpServer> {
    if def
        .get("type")
        .and_then(|v| v.as_str())
        .map(|t| t.eq_ignore_ascii_case("http"))
        .unwrap_or(false)
    {
        return None;
    }
    if def.get("url").is_some() {
        return None;
    }
    parse_stdio_like(key, def)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn mk_tmp_dir(prefix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let unique = format!("{}_{}", prefix, std::process::id());
        p.push(unique);
        // Best-effort cleanup if exists
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create tmp dir");
        p
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.sync_all().ok();
    }

    #[test]
    fn vscode_servers_key_is_parsed() {
        let ws = mk_tmp_dir("sb_vscode_servers");
        let mcp_path = ws.join(".vscode/mcp.json");
        let json = r#"{
            "servers": {
                "switchboard": { "command": "switchboard-mcp", "args": [], "env": {"RUST_LOG":"info"} }
            },
            "inputs": []
        }"#;
        write_file(&mcp_path, json);

        let discovered = discover_stdio_servers(&ws, None);
        let srv = discovered
            .by_key
            .get("switchboard")
            .expect("server present");
        match &srv.transport {
            McpTransport::Stdio { command, args, env } => {
                assert_eq!(command, "switchboard-mcp");
                assert!(args.is_empty());
                assert_eq!(env.get("RUST_LOG").map(|s| s.as_str()), Some("info"));
            }
        }
        assert!(matches!(srv.origin.provider, McpProvider::Vscode));
    }

    #[test]
    fn vscode_mcpservers_key_is_also_parsed() {
        let ws = mk_tmp_dir("sb_vscode_mcpservers");
        let mcp_path = ws.join(".vscode/mcp.json");
        let json = r#"{
            "mcpServers": {
                "switchboard": { "command": "switchboard-mcp", "args": ["--flag"], "env": {} }
            }
        }"#;
        write_file(&mcp_path, json);

        let discovered = discover_stdio_servers(&ws, None);
        let srv = discovered
            .by_key
            .get("switchboard")
            .expect("server present");
        match &srv.transport {
            McpTransport::Stdio {
                command,
                args,
                env: _,
            } => {
                assert_eq!(command, "switchboard-mcp");
                assert_eq!(args, &vec!["--flag".to_string()]);
            }
        }
        assert!(matches!(srv.origin.provider, McpProvider::Vscode));
    }
}
