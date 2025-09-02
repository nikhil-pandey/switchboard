use crate::mcp::types::{
    McpProvider, McpServerOrigin, McpToolRef, McpTransport, NormalizedMcpServer,
};

use super::types::{BuiltinToggle, LoadedMapping, MappingDest, ProviderId};
use crate::model::AgentConfig;

pub fn apply_mapping(
    provider: ProviderId,
    agent: &mut AgentConfig,
    mapping: &LoadedMapping,
    allow_custom_servers: bool,
) -> Vec<NormalizedMcpServer> {
    let mut custom_servers: Vec<NormalizedMcpServer> = Vec::new();
    let Some(refs) = agent.mcp_tool_refs.take() else {
        return custom_servers;
    };
    let (table, _name) = match provider {
        ProviderId::Vscode => (&mapping.vscode, "vscode"),
        ProviderId::Anthropic => (&mapping.anthropic, "anthropic"),
        ProviderId::Codex => (&mapping.anthropic, "codex"), // codex rarely uses mcp_tool_refs; reuse builtin mapping
    };

    let mut new_refs: Vec<McpToolRef> = Vec::new();
    let mut run = agent.run.take().unwrap_or_default();

    for r in refs {
        match r {
            McpToolRef::Namespaced { server_key, tool } => {
                // Already explicit; keep as-is
                new_refs.push(McpToolRef::Namespaced { server_key, tool });
            }
            McpToolRef::Bare { tool } => {
                // Resolve alias
                let key = table
                    .aliases
                    .get(&tool)
                    .cloned()
                    .unwrap_or_else(|| tool.clone());
                match table.map.get(&key) {
                    Some(MappingDest::Builtin(bt)) => match bt {
                        BuiltinToggle::Plan => run.include_plan_tool = Some(true),
                        BuiltinToggle::ApplyPatch => run.include_apply_patch_tool = Some(true),
                        BuiltinToggle::ViewImage => run.include_view_image_tool = Some(true),
                        BuiltinToggle::WebSearch => run.tools_web_search_request = Some(true),
                        BuiltinToggle::TerminalAccess => {
                            // Provided by default terminal; consume without toggles
                        }
                    },
                    Some(MappingDest::McpTool {
                        server_key,
                        tool,
                        command,
                        args,
                        env,
                    }) => {
                        new_refs.push(McpToolRef::Namespaced {
                            server_key: server_key.clone(),
                            tool: tool.clone(),
                        });
                        if allow_custom_servers {
                            custom_servers.push(NormalizedMcpServer {
                                key: server_key.clone(),
                                transport: McpTransport::Stdio {
                                    command: command.clone(),
                                    args: args.clone(),
                                    env: env.clone(),
                                },
                                origin: McpServerOrigin {
                                    provider: McpProvider::Mapping,
                                    path: None,
                                    note: Some("toolmap default".to_string()),
                                },
                            });
                        }
                    }
                    None => {
                        // Unknown; keep as bare for now
                        new_refs.push(McpToolRef::Bare { tool: key });
                    }
                }
            }
        }
    }

    agent.mcp_tool_refs = if new_refs.is_empty() {
        None
    } else {
        Some(new_refs)
    };
    agent.run = Some(run);
    custom_servers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentConfig;

    fn stub_agent_with_tools(tools: Vec<&str>) -> AgentConfig {
        AgentConfig {
            name: "t".into(),
            description: "d".into(),
            tags: None,
            toggles: None,
            mcp_tool_refs: Some(
                tools
                    .into_iter()
                    .map(|t| McpToolRef::Bare {
                        tool: t.to_string(),
                    })
                    .collect(),
            ),
            instructions_file: None,
            instructions: None,
            run: None,
            mcp_servers: None,
        }
    }

    #[test]
    fn vscode_edit_search_map_to_toggles() {
        let mapping = super::super::types::default_mapping();
        let mut agent = stub_agent_with_tools(vec!["edit", "search"]);
        let servers = apply_mapping(ProviderId::Vscode, &mut agent, &mapping, false);
        assert!(servers.is_empty());
        let run = agent.run.unwrap();
        assert_eq!(run.include_apply_patch_tool, Some(true));
        assert_eq!(run.tools_web_search_request, Some(true));
        assert!(agent.mcp_tool_refs.is_none());
    }

    #[test]
    fn anthropic_aliases_map_to_apply_patch_web_search_plan() {
        let mapping = super::super::types::default_mapping();
        let mut agent = stub_agent_with_tools(vec!["Edit", "WebFetch", "TodoWrite"]);
        let _ = apply_mapping(ProviderId::Anthropic, &mut agent, &mapping, false);
        let run = agent.run.unwrap();
        assert_eq!(run.include_apply_patch_tool, Some(true));
        assert_eq!(run.tools_web_search_request, Some(true));
        assert_eq!(run.include_plan_tool, Some(true));
        assert!(agent.mcp_tool_refs.is_none());
    }

    #[test]
    fn unmapped_vscode_tool_kept_as_bare() {
        let mapping = super::super::types::default_mapping();
        let mut agent = stub_agent_with_tools(vec!["runCommands"]);
        let _ = apply_mapping(ProviderId::Vscode, &mut agent, &mapping, false);
        // runCommands is provided by default terminal → consumed; no refs left
        assert!(agent.mcp_tool_refs.is_none());
        let run = agent.run.unwrap();
        assert_eq!(run.include_apply_patch_tool, None);
        assert_eq!(run.tools_web_search_request, None);
    }

    #[test]
    fn claude_terminal_tools_are_consumed_without_toggles() {
        let mapping = super::super::types::default_mapping();
        let mut agent = stub_agent_with_tools(vec!["Bash", "Grep"]);
        let _ = apply_mapping(ProviderId::Anthropic, &mut agent, &mapping, false);
        assert!(agent.mcp_tool_refs.is_none());
        let run = agent.run.unwrap();
        assert_eq!(run.include_apply_patch_tool, None);
        assert_eq!(run.tools_web_search_request, None);
        assert_eq!(run.include_plan_tool, None);
    }
}
