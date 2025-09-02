//! Shared types for MCP server discovery and tool references.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Transport options supported by this module. Currently stdio only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    // HTTP intentionally omitted; add when needed.
}

/// Normalized server definition with origin metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedMcpServer {
    pub key: String,
    pub transport: McpTransport,
    pub origin: McpServerOrigin,
}

/// Discovered servers keyed by name.
#[derive(Debug, Clone, Default)]
pub struct DiscoveredServers {
    pub by_key: HashMap<String, NormalizedMcpServer>,
}

/// Reference to an MCP tool by bare name or namespaced `server::tool`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpToolRef {
    Bare { tool: String },
    Namespaced { server_key: String, tool: String },
}

/// Origin/provider where a server definition was found.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum McpProvider {
    Claude,
    Vscode,
    Cursor,
    Mapping,
}

/// Origin metadata including source path and human note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerOrigin {
    pub provider: McpProvider,
    pub path: Option<PathBuf>,
    pub note: Option<String>,
}
