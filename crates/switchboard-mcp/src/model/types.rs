use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// Typed enums from codex for clarity and validation
use crate::mcp::types::McpToolRef;
use codex_core::config_types::Verbosity;
use codex_core::protocol::AskForApproval;
use codex_protocol::config_types::{ReasoningEffort, ReasoningSummary, SandboxMode};
use toml::Value as TomlValue;

/// Typed run settings that map to `profiles.<safe>.*` keys in Codex config.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRun {
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub approval_policy: Option<AskForApproval>,
    pub disable_response_storage: Option<bool>,
    pub model_reasoning_effort: Option<ReasoningEffort>,
    pub model_reasoning_summary: Option<ReasoningSummary>,
    pub model_verbosity: Option<Verbosity>,
    pub chatgpt_base_url: Option<String>,
    pub sandbox_mode: Option<SandboxMode>,
    pub include_plan_tool: Option<bool>,
    pub include_apply_patch_tool: Option<bool>,
    pub include_view_image_tool: Option<bool>,
    pub tools_web_search_request: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    /// Optional tags for the agent (top-level)
    pub tags: Option<Vec<String>>,
    /// Optional toggle policy for built-in Codex tools.
    pub toggles: Option<AgentTogglePolicy>,
    /// Optional MCP tool references for Claude/VSCode style tool lists.
    pub mcp_tool_refs: Option<Vec<McpToolRef>>,
    /// If set, points to a file with the agent instructions.
    pub instructions_file: Option<PathBuf>,
    /// Inline instructions for the agent when `instructions_file` is not used.
    pub instructions: Option<String>,
    /// Run settings (mapped under `profiles.<safe>.*`).
    pub run: Option<AgentRun>,
    /// Optional MCP server definitions passed as top-level overrides (codex only).
    pub mcp_servers: Option<TomlValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentTogglePolicy {
    pub include_plan_tool: Option<bool>,
    pub include_apply_patch_tool: Option<bool>,
    pub include_view_image_tool: Option<bool>,
    pub tools_web_search_request: Option<bool>,
}
