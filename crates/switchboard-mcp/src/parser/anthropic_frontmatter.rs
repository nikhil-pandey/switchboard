//! Parser for Anthropic-style `.agent.md` files with YAML frontmatter.

use anyhow::Context as _;
use serde::Deserialize;
use std::path::Path;

use crate::mcp::types::McpToolRef;
use crate::model::{AgentConfig, AgentRun};

use super::AgentParser;

/// Parser for `.agent.md` files.
pub struct AnthropicFrontmatterParser;

#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    tools: ToolsField,
    #[serde(default)]
    tags: ToolsField,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, alias = "provider", alias = "modelProvider")]
    model_provider: Option<String>,
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
                // For tools, split on commas and whitespace for convenience.
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

    fn into_vec_commas_only(self) -> Option<Vec<String>> {
        match self {
            ToolsField::List(v) => {
                if v.is_empty() {
                    None
                } else {
                    Some(v)
                }
            }
            ToolsField::Single(s) => {
                // For tags, allow multi-word items; split only on commas.
                let v: Vec<String> = s
                    .split(',')
                    .map(|t| t.trim())
                    .filter(|t| !t.is_empty())
                    .map(|t| t.to_string())
                    .collect();
                if v.is_empty() { None } else { Some(v) }
            }
            ToolsField::Empty => None,
        }
    }
}

impl AgentParser for AnthropicFrontmatterParser {
    fn supports(path: &Path) -> bool {
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
            return false;
        };
        fname.ends_with(".agent.md")
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
        let tags = fm.tags.into_vec_commas_only();
        let refs: Option<Vec<McpToolRef>> = fm.tools.into_vec().map(|v| {
            v.into_iter()
                .map(|t| McpToolRef::Bare { tool: t })
                .collect()
        });

        // The rest of the file after fm_end_idx is the instructions body
        let body: String = content
            .lines()
            .skip(fm_end_idx + 1)
            .collect::<Vec<&str>>()
            .join("\n");

        // Optional run mapping (model only; provider left unset for Codex defaults)
        let run = if fm.model.is_some() || fm.model_provider.is_some() {
            Some(AgentRun {
                model: fm.model,
                model_provider: fm.model_provider,
                ..Default::default()
            })
        } else {
            None
        };

        Ok(AgentConfig {
            name: fm.name,
            description: fm.description,
            tags,
            toggles: None,
            mcp_tool_refs: refs,
            instructions_file: None,
            instructions: Some(body.trim().to_string()),
            run,
            mcp_servers: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags_commas_only_preserves_spaces() {
        let content = r#"---
name: Example
description: Test
tags: deep research, demo, another tag
---
body
"#;
        let path = std::path::Path::new("/tmp/example.agent.md");
        let cfg = AnthropicFrontmatterParser::parse(content, path).expect("parse ok");
        let tags = cfg.tags.expect("tags");
        assert_eq!(tags, vec!["deep research", "demo", "another tag"]);
        assert_eq!(cfg.name, "Example");
        assert_eq!(cfg.description, "Test");
        assert_eq!(cfg.instructions.as_deref(), Some("body"));
    }

    #[test]
    fn parse_tools_single_and_list() {
        let content = r#"---
name: ToolsSingle
tools: plan apply_patch
---
body
"#;
        let path = std::path::Path::new("/tmp/tools_single.agent.md");
        let cfg = AnthropicFrontmatterParser::parse(content, path).expect("parse ok");
        let refs = cfg.mcp_tool_refs.expect("refs");
        let tools: Vec<String> = refs
            .into_iter()
            .map(|r| match r {
                McpToolRef::Bare { tool } => tool,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(tools, vec!["plan", "apply_patch"]);

        let content2 = r#"---
name: ToolsList
tools:
  - view_image
  - web_search
---
body
"#;
        let path2 = std::path::Path::new("/tmp/tools_list.agent.md");
        let cfg2 = AnthropicFrontmatterParser::parse(content2, path2).expect("parse ok");
        let refs2 = cfg2.mcp_tool_refs.expect("refs");
        let tools2: Vec<String> = refs2
            .into_iter()
            .map(|r| match r {
                McpToolRef::Bare { tool } => tool,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(tools2, vec!["view_image", "web_search"]);
    }

    #[test]
    fn parse_model_is_mapped_into_run() {
        let content = r#"---
name: WithModel
description: Test
model: sonnet
---
body
"#;
        let path = std::path::Path::new("/tmp/with_model.agent.md");
        let cfg = AnthropicFrontmatterParser::parse(content, path).expect("parse ok");
        assert_eq!(cfg.name, "WithModel");
        let run = cfg.run.expect("run present");
        assert_eq!(run.model.as_deref(), Some("sonnet"));
    }
}
