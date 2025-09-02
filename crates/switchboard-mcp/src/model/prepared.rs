//! Prepared agent configuration ready for execution.

use std::collections::HashMap;

use codex_core::config_types::McpServerConfig;

use super::naming::AgentVariant;
use super::types::AgentRun;

/// A fully prepared agent definition bound to a concrete tool name.
#[derive(Debug, Clone)]
pub struct PreparedAgent {
    pub tool_name: String,
    pub name: String,
    pub description: String,
    pub tags: Option<Vec<String>>,
    pub provider: AgentVariant,
    pub mcp_servers: HashMap<String, McpServerConfig>,
    pub instructions: Option<String>,
    pub run: Option<AgentRun>,
}
