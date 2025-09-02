//! Lightweight stdio MCP server tool enumerator.
//!
//! Connects to a server via stdio transport, initializes a client, fetches the
//! list of tools, and shuts down within a configurable timeout.

use std::collections::HashSet;
use std::time::Duration;

use rust_mcp_sdk::McpClient;
use rust_mcp_sdk::mcp_client::{ClientHandlerCore, client_runtime_core};
use rust_mcp_sdk::schema::schema_utils::{
    NotificationFromServer, RequestFromServer, ResultFromClient,
};
use rust_mcp_sdk::schema::{
    ClientCapabilities, Implementation, InitializeRequestParams, LATEST_PROTOCOL_VERSION, RpcError,
};
use rust_mcp_sdk::{StdioTransport, TransportOptions};

use super::{McpTransport, NormalizedMcpServer};

/// Result of enumerating a single server's tools.
#[derive(Debug, Clone)]
pub struct ServerTools {
    pub key: String,
    pub tools: HashSet<String>,
}

/// Enumerate tools from a stdio MCP server with a timeout.
pub async fn enumerate_stdio(
    server: &NormalizedMcpServer,
    timeout: Duration,
) -> anyhow::Result<ServerTools> {
    let (command, args, env) = match &server.transport {
        McpTransport::Stdio { command, args, env } => (command, args, env),
    };

    let client_details: InitializeRequestParams = InitializeRequestParams {
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "switchboard-mcp-enumerator".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            title: None,
        },
        protocol_version: LATEST_PROTOCOL_VERSION.into(),
    };

    let transport = StdioTransport::create_with_server_launch(
        command,
        args.clone(),
        Some(env.clone()),
        TransportOptions::default(),
    )
    .map_err(|e| anyhow::anyhow!(format!("transport error: {}", e)))?;

    let handler = NoopClientHandler;
    let client = client_runtime_core::create_client(client_details, transport, handler);

    // start
    tokio::time::timeout(timeout, client.clone().start())
        .await
        .map_err(|_| anyhow::anyhow!("enumeration start timeout"))
        .and_then(|r| r.map_err(|e| anyhow::anyhow!(format!("start error: {}", e))))?;

    // list tools
    let tools_resp = tokio::time::timeout(timeout, client.list_tools(None))
        .await
        .map_err(|_| anyhow::anyhow!("enumeration list_tools timeout"))
        .and_then(|r| r.map_err(|e| anyhow::anyhow!(format!("list_tools error: {}", e))))?;
    let tools: HashSet<String> = tools_resp.tools.into_iter().map(|t| t.name).collect();

    // shutdown
    tokio::time::timeout(timeout, client.shut_down())
        .await
        .map_err(|_| anyhow::anyhow!("enumeration shutdown timeout"))
        .and_then(|r| r.map_err(|e| anyhow::anyhow!(format!("shutdown error: {}", e))))?;

    Ok(ServerTools {
        key: server.key.clone(),
        tools,
    })
}

#[derive(Clone)]
struct NoopClientHandler;

#[async_trait::async_trait]
impl ClientHandlerCore for NoopClientHandler {
    async fn handle_request(
        &self,
        _request: RequestFromServer,
        _runtime: &dyn McpClient,
    ) -> std::result::Result<ResultFromClient, RpcError> {
        Err(RpcError::method_not_found())
    }

    async fn handle_notification(
        &self,
        _notification: NotificationFromServer,
        _runtime: &dyn McpClient,
    ) -> std::result::Result<(), RpcError> {
        Ok(())
    }

    async fn handle_error(
        &self,
        _error: &RpcError,
        _runtime: &dyn McpClient,
    ) -> std::result::Result<(), RpcError> {
        Ok(())
    }
}
