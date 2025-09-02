//! Parser for Codex `.toml` agent definitions.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use toml::Value as TomlValue;

use crate::model::{AgentConfig, AgentRun, AgentTogglePolicy};

use super::AgentParser;

/// Parser for codex agent TOML files.
pub struct CodexTomlParser;

impl AgentParser for CodexTomlParser {
    fn supports(path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("toml"))
            .unwrap_or(false)
    }

    fn parse(content: &str, path: &Path) -> anyhow::Result<AgentConfig> {
        // Strict TOML: entire file must be valid TOML with a table at root
        let mut tbl: toml::Table = content.parse().context("invalid TOML")?;

        // Required name
        let name = tbl
            .remove("name")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| anyhow::anyhow!("missing 'name' in {}", path.display()))?;

        // Optional description
        let description = tbl
            .remove("description")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "Agent".to_string());

        // Tags (top-level): array or string
        let tags: Option<Vec<String>> = match tbl.remove("tags") {
            Some(TomlValue::Array(items)) => {
                let v: Vec<String> = items
                    .into_iter()
                    .filter_map(|it| it.as_str().map(|s| s.to_string()))
                    .collect();
                if v.is_empty() { None } else { Some(v) }
            }
            Some(TomlValue::String(s)) => {
                let v: Vec<String> = s
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| t.trim().to_string())
                    .collect();
                if v.is_empty() { None } else { Some(v) }
            }
            Some(other) => {
                tracing::warn!(
                    "unexpected 'tags' type in {}: {}",
                    path.display(),
                    other.type_str()
                );
                None
            }
            None => None,
        };

        // Instructions: prefer explicit instructions_file, then inline `instructions`,
        // otherwise fall back to sibling <file>.prompt.md.
        let instructions_file = match tbl.remove("instructions_file") {
            Some(TomlValue::String(s)) => Some(PathBuf::from(s)),
            Some(other) => {
                tracing::warn!(
                    "ignoring non-string 'instructions_file' in {} (found: {})",
                    path.display(),
                    other.type_str()
                );
                None
            }
            None => None,
        };
        let mut instructions = tbl
            .remove("instructions")
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        if instructions_file.is_none() && instructions.is_none() {
            let prompt_path = path.with_extension("prompt.md");
            if prompt_path.exists() {
                match std::fs::read_to_string(&prompt_path) {
                    Ok(s) => instructions = Some(s),
                    Err(e) => {
                        tracing::warn!(
                            "failed to read prompt file {}: {} (using empty instructions)",
                            prompt_path.display(),
                            e
                        );
                    }
                }
            }
        }

        // Run settings under [run]
        let run: Option<AgentRun> = match tbl.remove("run") {
            Some(TomlValue::Table(t)) => match t.try_into() {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!("invalid [run] in {}: {}", path.display(), e);
                    None
                }
            },
            Some(other) => {
                tracing::warn!(
                    "ignoring non-table [run] in {} (found: {})",
                    path.display(),
                    other.type_str()
                );
                None
            }
            None => None,
        };

        // Optional mcp_servers table (codex only)
        let mcp_servers = tbl.remove("mcp_servers");

        // Optional tools (array or string) → toggles policy
        let toggles: Option<AgentTogglePolicy> = match tbl.remove("tools") {
            Some(TomlValue::Array(items)) => Some(map_tools_to_toggles(
                items
                    .into_iter()
                    .filter_map(|it| it.as_str().map(|s| s.to_string()))
                    .collect(),
            )),
            Some(TomlValue::String(s)) => Some(map_tools_to_toggles(
                s.split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| t.trim().to_string())
                    .collect(),
            )),
            Some(other) => {
                tracing::warn!(
                    "unexpected 'tools' type in {}: {}",
                    path.display(),
                    other.type_str()
                );
                None
            }
            None => None,
        };

        // Ignore stray keys but log
        if !tbl.is_empty() {
            tracing::debug!(
                "unrecognized keys in {}: {}",
                path.display(),
                tbl.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }

        Ok(AgentConfig {
            name,
            description,
            tags,
            toggles,
            mcp_tool_refs: None,
            instructions_file,
            instructions,
            run,
            mcp_servers,
        })
    }
}

fn map_tools_to_toggles(list: Vec<String>) -> AgentTogglePolicy {
    let has = |needle: &str| list.iter().any(|s| s.eq_ignore_ascii_case(needle));
    AgentTogglePolicy {
        include_plan_tool: Some(has("plan")),
        include_apply_patch_tool: Some(has("apply_patch") || has("apply-patch")),
        include_view_image_tool: Some(has("view_image") || has("view-image")),
        tools_web_search_request: Some(has("web_search") || has("web-search")),
    }
}
