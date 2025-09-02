//! Agent source and resolution details prior to preparation.

use std::path::PathBuf;

use super::{AgentConfig, AgentVariant};
use crate::mcp::types::NormalizedMcpServer;

/// Source attachment for an agent (provider and origin path).
#[derive(Debug, Clone)]
pub struct AgentSource {
    pub variant: AgentVariant,
    #[allow(dead_code)]
    pub path: PathBuf,
}

/// A resolved agent prior to converting MCP servers and instructions.
#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub source: AgentSource,
    pub config: AgentConfig,
    pub tool_name: String,
    pub mcp_servers: std::collections::HashMap<String, NormalizedMcpServer>,
}
