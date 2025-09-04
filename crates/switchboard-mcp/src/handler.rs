//! MCP server handler that exposes prepared agents as callable tools.
//!
//! This handler validates protocol initialization, lists tools derived from
//! discovered/prepared agents, and routes `call_tool` requests to the in-process
//! Codex runner.

use std::collections::HashMap;

use async_trait::async_trait;
use rust_mcp_sdk::schema::{
    ClientRequest, ListToolsResult, RpcError, TextContent, Tool, ToolInputSchema,
    schema_utils::{NotificationFromClient, RequestFromClient, ResultFromServer},
};
use rust_mcp_sdk::{
    McpServer,
    mcp_server::{ServerHandlerCore, enforce_compatible_protocol_version},
};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
// no external process management here; runners handle process or in-proc logic

use crate::codex_runner::{CodexRunner, InprocCodexRunner};
use crate::model::PreparedAgent;

// No external `codex` binary usage; always run Codex in-process.

/// Routes MCP requests and maintains an index of agent tool definitions.
pub struct AgentsServerHandler {
    /// Mapping from tool name to prepared agent configuration.
    agents_by_tool: HashMap<String, PreparedAgent>,
}

impl AgentsServerHandler {
    pub fn new(agents: Vec<PreparedAgent>) -> Self {
        let mut map = HashMap::new();
        for a in agents {
            map.insert(a.tool_name.clone(), a);
        }
        tracing::debug!("initialized AgentsServerHandler (tools={})", map.len());
        Self {
            agents_by_tool: map,
        }
    }

    /// Build the list of tool definitions exposed by this server.
    fn tool_definitions(&self) -> Vec<Tool> {
        tracing::debug!(
            "building tool definitions (count={})",
            self.agents_by_tool.len()
        );
        self.agents_by_tool
            .iter()
            .map(|(tool_name, ra)| {
                tracing::debug!("registering tool {} from {:?}", tool_name, ra.provider);
                // Build a friendly description including tags and the input param name
                let desc = if let Some(tags) = &ra.tags {
                    let t = if tags.is_empty() {
                        String::new()
                    } else {
                        tags.join(", ")
                    };
                    if t.is_empty() {
                        format!("task, cwd: string — {}", ra.description)
                    } else {
                        format!("task, cwd: string — {} [tags: {}]", ra.description, t)
                    }
                } else {
                    format!("task, cwd: string — {}", ra.description)
                };
                // Input schema: { task: string, cwd: string }
                let mut props = HashMap::<String, JsonMap<String, JsonValue>>::new();
                let mut task_schema = JsonMap::new();
                task_schema.insert("type".to_string(), JsonValue::String("string".to_string()));
                task_schema.insert(
                    "description".to_string(),
                    JsonValue::String("Task to perform".to_string()),
                );
                props.insert("task".to_string(), task_schema);
                let mut cwd_schema = JsonMap::new();
                cwd_schema.insert("type".to_string(), JsonValue::String("string".to_string()));
                cwd_schema.insert(
                    "description".to_string(),
                    JsonValue::String("Working directory (must be an absolute path).".to_string()),
                );
                props.insert("cwd".to_string(), cwd_schema);
                Tool {
                    annotations: None,
                    description: Some(desc),
                    input_schema: ToolInputSchema::new(
                        vec!["task".to_string(), "cwd".to_string()],
                        Some(props),
                    ),
                    meta: None,
                    name: tool_name.clone(),
                    output_schema: None,
                    title: None,
                }
            })
            .collect()
    }
}

#[async_trait]
impl ServerHandlerCore for AgentsServerHandler {
    async fn handle_request(
        &self,
        request: RequestFromClient,
        runtime: &dyn McpServer,
    ) -> std::result::Result<ResultFromServer, RpcError> {
        let method_name = request.method().to_owned();
        tracing::info!("handle_request: method={}", method_name);
        match request {
            RequestFromClient::ClientRequest(client_request) => match client_request {
                // Initialize: enforce protocol compatibility and return server info
                ClientRequest::InitializeRequest(initialize_request) => {
                    tracing::debug!(
                        "initialize_request: client_protocol={}",
                        initialize_request.params.protocol_version
                    );
                    let mut server_info = runtime.server_info().to_owned();
                    if let Some(updated_protocol_version) = enforce_compatible_protocol_version(
                        &initialize_request.params.protocol_version,
                        &server_info.protocol_version,
                    )
                    .map_err(|err| {
                        tracing::error!(
                            "incompatible protocol version (client={}, server={})",
                            initialize_request.params.protocol_version,
                            server_info.protocol_version
                        );
                        RpcError::internal_error().with_message(err.to_string())
                    })? {
                        server_info.protocol_version = updated_protocol_version;
                    }
                    tracing::info!("initialized (protocol={})", server_info.protocol_version);
                    Ok(server_info.into())
                }

                // List tools
                ClientRequest::ListToolsRequest(_) => {
                    let tools = self.tool_definitions();
                    tracing::info!("list_tools (count={})", tools.len());
                    Ok(ListToolsResult {
                        meta: None,
                        next_cursor: None,
                        tools,
                    }
                    .into())
                }

                // Call tool
                ClientRequest::CallToolRequest(request) => {
                    let tool = request.tool_name().to_string();
                    // Snapshot argument keys early for better diagnostics
                    let arg_keys = request
                        .params
                        .arguments
                        .as_ref()
                        .map(|m| m.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    tracing::info!("call_tool request: tool={}, arg_keys={:?}", tool, arg_keys);
                    let Some(ra) = self.agents_by_tool.get(&tool) else {
                        tracing::warn!("unknown tool: {}", tool);
                        return Err(RpcError::method_not_found()
                            .with_message(format!("Unknown tool '{}'", tool)));
                    };

                    // Extract required arguments: task, cwd
                    let task = request
                        .params
                        .arguments
                        .as_ref()
                        .and_then(|m| m.get("task"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            let arg_dbg = request
                                .params
                                .arguments
                                .as_ref()
                                .map(|m| serde_json::to_string(m).unwrap_or_default())
                                .unwrap_or_else(|| "<none>".to_string());
                            tracing::error!(
                                "missing required 'task' in arguments (tool={}; args={})",
                                tool,
                                arg_dbg
                            );
                            RpcError::internal_error()
                                .with_message("missing required 'task' string".to_string())
                        })?;
                    let cwd = request
                        .params
                        .arguments
                        .as_ref()
                        .and_then(|m| m.get("cwd"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            let arg_dbg = request
                                .params
                                .arguments
                                .as_ref()
                                .map(|m| serde_json::to_string(m).unwrap_or_default())
                                .unwrap_or_else(|| "<none>".to_string());
                            tracing::error!(
                                "missing required 'cwd' in arguments (tool={}; args={})",
                                tool,
                                arg_dbg
                            );
                            RpcError::internal_error()
                                .with_message("missing required 'cwd' string".to_string())
                        })?;
                    // Enforce absolute cwd to avoid ambiguous resolution.
                    let cwd_is_abs = std::path::Path::new(cwd).is_absolute();
                    if !cwd_is_abs {
                        tracing::error!(
                            tool = %tool,
                            cwd_value = %cwd,
                            cwd_is_abs = %cwd_is_abs,
                            os = %std::env::consts::OS,
                            "invalid cwd (must be absolute)"
                        );
                        return Err(
                            rust_mcp_sdk::schema::RpcError::invalid_params().with_message(format!(
                                "invalid 'cwd': got '{cwd}', expected an absolute path"
                            )),
                        );
                    }
                    tracing::debug!(
                        tool = %tool,
                        task_len = %task.chars().count(),
                        agent = %ra.name,
                        cwd = %cwd,
                        "invoking agent"
                    );

                    // Always use the in-process Codex runner
                    let runner = InprocCodexRunner::new();
                    let result = match runner.exec_task(ra, &tool, task, cwd).await {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::error!("codex execution failed: {}", e);
                            let payload = json!({
                                "ok": false,
                                "output": "",
                            });
                            return Ok(rust_mcp_sdk::schema::CallToolResult::text_content(vec![
                                TextContent::from(payload.to_string()),
                            ])
                            .into());
                        }
                    };

                    // Log stderr (debug) instead of returning it in payload
                    if !result.stderr.is_empty() {
                        tracing::debug!(
                            "codex stderr ({} bytes):\n{}",
                            result.stderr.len(),
                            result.stderr
                        );
                    }

                    let payload = json!({
                        "ok": result.ok,
                        "output": result.stdout,
                    });
                    if result.ok {
                        tracing::info!("codex finished successfully (code={})", result.status);
                    } else {
                        tracing::warn!("codex exited with non-zero code (code={})", result.status);
                    }
                    Ok(
                        rust_mcp_sdk::schema::CallToolResult::text_content(vec![
                            TextContent::from(payload.to_string()),
                        ])
                        .into(),
                    )
                }

                _ => {
                    tracing::warn!("method not implemented: {}", method_name);
                    Err(RpcError::method_not_found()
                        .with_message(format!("No handler is implemented for '{method_name}'.")))
                }
            },
            RequestFromClient::CustomRequest(_) => {
                tracing::warn!("custom request not implemented");
                Err(RpcError::method_not_found()
                    .with_message("No handler is implemented for custom requests.".to_string()))
            }
        }
    }

    async fn handle_notification(
        &self,
        notification: NotificationFromClient,
        _: &dyn McpServer,
    ) -> std::result::Result<(), RpcError> {
        match &notification {
            rust_mcp_sdk::schema::schema_utils::NotificationFromClient::ClientNotification(_) => {
                tracing::debug!("handle_notification: client notification")
            }
            rust_mcp_sdk::schema::schema_utils::NotificationFromClient::CustomNotification(_) => {
                tracing::debug!("handle_notification: custom notification")
            }
        }
        Ok(())
    }

    async fn handle_error(
        &self,
        error: &RpcError,
        _: &dyn McpServer,
    ) -> std::result::Result<(), RpcError> {
        tracing::error!(
            "handle_error from client (code={:?}, message={:?})",
            error.code,
            error.message
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PreparedAgent, naming::AgentVariant};
    use std::collections::HashMap as Map;

    fn sample_agent() -> PreparedAgent {
        PreparedAgent {
            tool_name: "agent_sample".to_string(),
            name: "Sample".to_string(),
            description: "Sample agent".to_string(),
            tags: None,
            provider: AgentVariant::Codex,
            mcp_servers: Map::new(),
            instructions: None,
            run: None,
        }
    }

    #[test]
    fn tool_schema_includes_cwd_required() {
        let h = AgentsServerHandler::new(vec![sample_agent()]);
        let tools = h.tool_definitions();
        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        // Serialize to JSON to inspect schema details without relying on field visibility
        let val = serde_json::to_value(tool).expect("serialize tool");
        eprintln!("tool json: {}", serde_json::to_string_pretty(&val).unwrap());
        // required contains both task and cwd
        let req = val
            .get("inputSchema")
            .and_then(|s| s.get("required"))
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        let req_set: std::collections::HashSet<String> = req
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        assert!(req_set.contains("task"), "required should include 'task'");
        assert!(req_set.contains("cwd"), "required should include 'cwd'");
    }
}
