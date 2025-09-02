//! Agent config parsers for different provider formats.
//!
//! Each parser declares a `supports` predicate over file paths and a `parse`
//! function that returns a normalized `AgentConfig`.

use std::path::Path;

use anyhow::Result;

use crate::model::AgentConfig;

/// Parser trait implemented by provider-specific formats.
pub trait AgentParser {
    fn supports(path: &Path) -> bool;
    fn parse(content: &str, path: &Path) -> Result<AgentConfig>;
}

pub mod anthropic_frontmatter;
pub mod codex_toml;
pub mod vscode_chatmode;
