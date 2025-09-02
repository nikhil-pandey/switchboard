//! Parser for VSCode `.chatmode.md` files with YAML frontmatter.

use anyhow::Context as _;
use serde::Deserialize;
use std::path::Path;

use crate::mcp::types::McpToolRef;
use crate::model::{AgentConfig, AgentRun};

use super::AgentParser;

/// Parser for VSCode chatmode files.
pub struct VscodeChatmodeParser;

#[derive(Debug, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    name: Option<String>,
    description: String,
    #[serde(default)]
    tools: ToolsField, // VSCode tools are metadata; do not map to Codex toggles
    #[serde(default)]
    model: Option<String>, // metadata only
    #[serde(default, alias = "provider", alias = "modelProvider")]
    model_provider: Option<String>, // optional provider
    #[serde(default)]
    tags: ToolsField, // optional metadata (string or list)
}

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum ToolsField {
    List(Vec<String>),
    Single(String),
    #[default]
    Empty,
}

impl ToolsField {
    fn into_vec(self) -> Option<Vec<String>> {
        match self {
            ToolsField::List(v) => {
                if v.is_empty() {
                    None
                } else {
                    Some(v)
                }
            }
            ToolsField::Single(s) => {
                let v: Vec<String> = s
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| t.trim().to_string())
                    .collect();
                if v.is_empty() { None } else { Some(v) }
            }
            ToolsField::Empty => None,
        }
    }
}

impl AgentParser for VscodeChatmodeParser {
    fn supports(path: &Path) -> bool {
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
            return false;
        };
        fname.ends_with(".chatmode.md")
    }

    fn parse(content: &str, path: &Path) -> anyhow::Result<AgentConfig> {
        // Detect and split YAML frontmatter delimited by --- ... --- at file start
        let mut first_non_empty = None;
        for (idx, l) in content.lines().enumerate() {
            if !l.trim().is_empty() {
                first_non_empty = Some((idx, l));
                break;
            }
        }
        let Some((start_idx, first_line)) = first_non_empty else {
            return Err(anyhow::anyhow!("empty file: {}", path.display()));
        };
        if first_line.trim() != "---" {
            return Err(anyhow::anyhow!(
                "missing frontmatter '---' at start of {}",
                path.display()
            ));
        }

        // Collect lines after first '---' until next '---'
        let mut fm_end = None;
        let mut yaml_buf = String::new();
        for (i, l) in content.lines().enumerate().skip(start_idx + 1) {
            if l.trim() == "---" {
                fm_end = Some(i);
                break;
            }
            yaml_buf.push_str(l);
            yaml_buf.push('\n');
        }
        let Some(fm_end_idx) = fm_end else {
            return Err(anyhow::anyhow!(
                "unterminated frontmatter in {} (expected closing '---')",
                path.display()
            ));
        };

        let fm: Frontmatter =
            serde_yaml::from_str(&yaml_buf).context("invalid YAML frontmatter")?;
        // Touch fields to satisfy dead_code lint (metadata only).
        let _unused_tools = &fm.tools;
        let _unused_model = &fm.model;
        let name = fm.name.unwrap_or_else(|| derive_name_from_filename(path));
        let description = fm.description;

        // The rest of the file after fm_end_idx is the instructions body
        let body: String = content
            .lines()
            .skip(fm_end_idx + 1)
            .collect::<Vec<&str>>()
            .join("\n");

        // Build tool refs: either Bare or Namespaced
        let mcp_tool_refs: Option<Vec<McpToolRef>> = fm.tools.into_vec().map(|tools| {
            tools
                .into_iter()
                .map(|t| parse_vscode_tool_ref(&t))
                .collect()
        });

        // Optional run mapping (model only; provider left unset for Codex defaults)
        let run = if fm.model.is_some() || fm.model_provider.is_some() {
            Some(AgentRun {
                model: fm.model.clone(),
                model_provider: fm.model_provider.clone(),
                ..Default::default()
            })
        } else {
            None
        };

        Ok(AgentConfig {
            name,
            description,
            tags: fm.tags.into_vec(),
            toggles: None, // vscode tools are metadata, not codex toggles
            mcp_tool_refs,
            instructions_file: None,
            instructions: Some(body.trim().to_string()),
            run,
            mcp_servers: None,
        })
    }
}

fn derive_name_from_filename(path: &Path) -> String {
    let fname = path.file_stem().and_then(|s| s.to_str()).unwrap_or("agent");
    // file_stem() on .chatmode.md returns "<name>.chatmode"; trim suffix
    fname.strip_suffix(".chatmode").unwrap_or(fname).to_string()
}

fn parse_vscode_tool_ref(s: &str) -> McpToolRef {
    if let Some((server, tool)) = s.split_once("::") {
        McpToolRef::Namespaced {
            server_key: server.trim().to_string(),
            tool: tool.trim().to_string(),
        }
    } else {
        McpToolRef::Bare {
            tool: s.trim().to_string(),
        }
    }
}
