//! In-process Codex task runner and trait definition.
//!
//! This module provides a small abstraction (`CodexRunner`) to execute a task
//! with a prepared agent profile, plus a default in-process implementation
//! (`InprocCodexRunner`) that wires Codex Core directly without spawning a
//! separate process. Output is captured and returned in a structured form.

use crate::model::{PreparedAgent, safe_name};
use anyhow::{Context, anyhow};
use async_trait::async_trait;

/// Structured result of running a Codex task.
pub struct CodexRunOutput {
    /// True if the task reached ShutdownComplete without errors.
    pub ok: bool,
    /// Conventional exit status (0 on success, 1 on failure).
    pub status: i32,
    /// Aggregated agent message content and final output.
    pub stdout: String,
    /// Aggregated background/debug logs and error details.
    pub stderr: String,
}

#[async_trait]
pub trait CodexRunner: Send + Sync {
    /// Execute a task for a given prepared agent and capture its output.
    ///
    /// - `prepared`: fully resolved agent config (profile + MCP servers).
    /// - `tool`: logical tool name used by the caller (for logging/trace only).
    /// - `task`: user-provided task text.
    /// - `cwd`: working directory for execution.
    ///
    /// Returns aggregated stdout/stderr and status. Errors represent launch
    /// failures; nonzero status is not treated as an error.
    async fn exec_task(
        &self,
        prepared: &PreparedAgent,
        tool: &str,
        task: &str,
        cwd: &str,
    ) -> anyhow::Result<CodexRunOutput>;
}

/// Default in-process Codex runner.
pub struct InprocCodexRunner;

impl InprocCodexRunner {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CodexRunner for InprocCodexRunner {
    async fn exec_task(
        &self,
        prepared: &PreparedAgent,
        _tool: &str,
        task: &str,
        cwd: &str,
    ) -> anyhow::Result<CodexRunOutput> {
        let agent_name = &prepared.name;
        use codex_core::config::ConfigOverrides;
        use codex_core::protocol::{EventMsg, InputItem, Op, TaskCompleteEvent};
        use codex_core::{ConversationManager, NewConversation};
        use codex_login::AuthManager;

        // Log basic call context.
        let safe = safe_name(agent_name);
        tracing::info!(
            "in-proc codex: tool={}, agent={}, profile={}",
            _tool,
            agent_name,
            safe
        );
        tracing::debug!("task length (chars) = {}", task.len());

        // Build an in-memory ConfigToml with a derived profile and optional MCP servers.
        use codex_core::config::{ConfigToml, find_codex_home, load_config_as_toml};
        use codex_core::config_profile::ConfigProfile;

        let codex_home = find_codex_home().context("failed to resolve codex home")?;
        let root = load_config_as_toml(&codex_home)?;
        let mut cfg: ConfigToml = root
            .try_into()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            .context("failed to deserialize config.toml")?;

        // Prepare/override selected profile
        cfg.profile = Some(safe.clone());
        // Prepare an empty profile and only set values explicitly provided by the agent.
        let mut profile = ConfigProfile::default();

        if let Some(run) = &prepared.run {
            if let Some(v) = &run.model {
                profile.model = Some(v.clone());
            }
            if let Some(v) = &run.model_provider {
                profile.model_provider = Some(v.clone());
            }
            if let Some(v) = run.approval_policy {
                profile.approval_policy = Some(v);
            }
            if let Some(v) = run.disable_response_storage {
                profile.disable_response_storage = Some(v);
            }
            if let Some(v) = run.model_reasoning_effort {
                profile.model_reasoning_effort = Some(v);
            }
            if let Some(v) = run.model_reasoning_summary {
                profile.model_reasoning_summary = Some(v);
            }
            if let Some(v) = run.model_verbosity {
                profile.model_verbosity = Some(v);
            }
            if let Some(v) = &run.chatgpt_base_url {
                profile.chatgpt_base_url = Some(v.clone());
            }
        }
        cfg.profiles.insert(safe.clone(), profile);

        for (key, msc) in &prepared.mcp_servers {
            cfg.mcp_servers.insert(key.clone(), msc.clone());
        }

        // Minimal overrides: profile selection, cwd (force absolute), base_instructions, and select flags
        let mut overrides = ConfigOverrides {
            config_profile: Some(safe.clone()),
            cwd: Some({
                use std::path::PathBuf;
                let p = PathBuf::from(cwd);
                if p.as_os_str().is_empty() || !p.is_absolute() {
                    return Err(anyhow!("cwd must be an absolute path"));
                }
                p
            }),
            ..Default::default()
        };
        if let Some(s) = prepared.instructions.as_ref()
            && !s.trim().is_empty()
        {
            overrides.base_instructions = Some(s.clone());
        }
        if let Some(run) = &prepared.run {
            overrides.sandbox_mode = run.sandbox_mode;
            overrides.include_plan_tool = run.include_plan_tool;
            overrides.include_apply_patch_tool = run.include_apply_patch_tool;
            overrides.include_view_image_tool = run.include_view_image_tool;
            overrides.tools_web_search_request = run.tools_web_search_request;
        }

        let config = codex_core::config::Config::load_from_base_config_with_overrides(
            cfg, overrides, codex_home,
        )?;
        tracing::info!(
            "config: model={}, provider={}, cwd={}, disable_response_storage={}, show_raw_reasoning={}",
            config.model,
            config.model_provider_id,
            config.cwd.display(),
            config.disable_response_storage,
            config.show_raw_agent_reasoning
        );

        let conversation_manager = ConversationManager::new(AuthManager::shared(
            config.codex_home.clone(),
            config.preferred_auth_method,
        ));
        let NewConversation { conversation, .. } =
            conversation_manager.new_conversation(config).await?;
        tracing::debug!("conversation initialized");

        // Submit the task as user input.
        let items = vec![InputItem::Text {
            text: task.to_string(),
        }];
        let initial_id = conversation.submit(Op::UserInput { items }).await?;
        tracing::info!("submitted task event_id={}", initial_id);

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        let mut ok = false;

        // Drain events until shutdown.
        loop {
            let event = match conversation.next_event().await {
                Ok(ev) => ev,
                Err(e) => {
                    stderr_buf.push_str(&format!("error receiving event: {e}\n"));
                    break;
                }
            };

            match event.msg {
                EventMsg::TaskStarted(_) => {
                    tracing::info!("task started");
                }
                EventMsg::TaskComplete(TaskCompleteEvent { last_agent_message }) => {
                    if let Some(msg) = last_agent_message {
                        stdout_buf.push_str(&msg);
                        stdout_buf.push('\n');
                    }
                    // Initiate shutdown; then continue to read until ShutdownComplete.
                    tracing::info!("task_complete received; initiating shutdown");
                    conversation.submit(Op::Shutdown).await.ok();
                }
                EventMsg::AgentMessage(ev) => {
                    // Non-delta agent message (full chunk)
                    #[allow(clippy::let_and_return)]
                    let len = ev.message.len();
                    tracing::debug!("agent message len={}", len);
                }
                EventMsg::ShutdownComplete => {
                    ok = true;
                    tracing::info!("shutdown complete; exiting event loop");
                    break;
                }
                EventMsg::Error(err) => {
                    stderr_buf.push_str(&format!("error: {}\n", err.message));
                    tracing::warn!("event error: {}", err.message);
                }
                EventMsg::StreamError(err) => {
                    stderr_buf.push_str(&format!("stream_error: {}\n", err.message));
                    tracing::warn!("stream error: {}", err.message);
                }
                EventMsg::BackgroundEvent(ev) => {
                    // Treat as debug noise: accumulate into stderr to keep stdout clean for callers.
                    stderr_buf.push_str(&format!("{}\n", ev.message));
                    tracing::debug!("background: {}", ev.message);
                }
                EventMsg::TokenCount(token_usage) => {
                    tracing::info!("tokens used: {}", token_usage.blended_total());
                }
                EventMsg::AgentReasoningSectionBreak(_) => {
                    tracing::debug!("reasoning section break");
                }
                EventMsg::AgentReasoningRawContent(ev) => {
                    tracing::debug!("raw reasoning text len={}", ev.text.len());
                }
                EventMsg::AgentReasoningRawContentDelta(ev) => {
                    tracing::debug!("raw reasoning delta chars={}", ev.delta.len());
                }
                EventMsg::ExecCommandBegin(ev) => {
                    let cmd_preview = ev.command.join(" ");
                    let cwd = ev.cwd.display();
                    tracing::info!(
                        "exec begin call_id={}, cwd={}, cmd='{}'",
                        ev.call_id,
                        cwd,
                        cmd_preview
                    );
                }
                EventMsg::ExecCommandOutputDelta(_) => {
                    tracing::debug!("exec output delta received");
                }
                EventMsg::ExecCommandEnd(ev) => {
                    let code = ev.exit_code;
                    let dur_ms = ev.duration.as_millis();
                    tracing::info!(
                        "exec end call_id={}, exit_code={}, duration_ms={}",
                        ev.call_id,
                        code,
                        dur_ms
                    );
                }
                EventMsg::McpToolCallBegin(ev) => {
                    tracing::info!(
                        "mcp tool begin server={}, tool={} call_id={}",
                        ev.invocation.server,
                        ev.invocation.tool,
                        ev.call_id
                    );
                }
                EventMsg::McpToolCallEnd(ev) => {
                    let ok = ev.is_success();
                    let dur_ms = ev.duration.as_millis();
                    tracing::info!(
                        "mcp tool end server={}, tool={}, ok={}, duration_ms={}",
                        ev.invocation.server,
                        ev.invocation.tool,
                        ok,
                        dur_ms
                    );
                }
                EventMsg::WebSearchBegin(ev) => {
                    tracing::debug!("web search begin call_id={}", ev.call_id);
                }
                EventMsg::WebSearchEnd(ev) => {
                    tracing::info!(
                        "web search end call_id={}, query='{}'",
                        ev.call_id,
                        ev.query
                    );
                }
                EventMsg::PatchApplyBegin(ev) => {
                    // Count the number of files in the patch
                    let changes = ev.changes.len();
                    tracing::info!(
                        "apply_patch begin call_id={}, auto_approved={}, files={} ",
                        ev.call_id,
                        ev.auto_approved,
                        changes
                    );
                }
                EventMsg::PatchApplyEnd(ev) => {
                    tracing::info!(
                        "apply_patch end call_id={}, success={}, stdout_len={}, stderr_len={}",
                        ev.call_id,
                        ev.success,
                        ev.stdout.len(),
                        ev.stderr.len()
                    );
                }
                EventMsg::TurnDiff(ev) => {
                    tracing::debug!("turn diff len={}", ev.unified_diff.len());
                }
                EventMsg::ExecApprovalRequest(_) => {
                    tracing::info!("exec approval requested");
                }
                EventMsg::ApplyPatchApprovalRequest(_) => {
                    tracing::info!("apply_patch approval requested");
                }
                EventMsg::AgentReasoning(ev) => {
                    tracing::debug!("agent reasoning text len={}", ev.text.len());
                }
                EventMsg::SessionConfigured(ev) => {
                    tracing::info!(
                        "session configured: session_id={}, model={}",
                        ev.session_id,
                        ev.model
                    );
                }
                EventMsg::PlanUpdate(ev) => {
                    let steps = ev.plan.len();
                    let explanation_len = ev.explanation.as_deref().map(|s| s.len()).unwrap_or(0);
                    tracing::debug!(
                        "plan update: steps={}, explanation_len={}",
                        steps,
                        explanation_len
                    );
                }
                EventMsg::GetHistoryEntryResponse(_) => {
                    tracing::debug!("history entry response received");
                }
                EventMsg::McpListToolsResponse(_) => {
                    tracing::debug!("mcp list tools response received");
                }
                EventMsg::ListCustomPromptsResponse(_) => {
                    tracing::debug!("list custom prompts response received");
                }
                EventMsg::TurnAborted(reason) => match reason.reason {
                    codex_core::protocol::TurnAbortReason::Interrupted => {
                        tracing::warn!("task interrupted")
                    }
                    codex_core::protocol::TurnAbortReason::Replaced => {
                        tracing::warn!("task aborted: replaced by new task")
                    }
                },
                _ => {
                    // Ignore other events for now.
                }
            }
        }

        let status_code = if ok { 0 } else { 1 };
        tracing::info!(
            "in-proc codex finished: ok={}, status={}, stdout_len={}, stderr_len={}",
            ok,
            status_code,
            stdout_buf.len(),
            stderr_buf.len()
        );
        Ok(CodexRunOutput {
            ok,
            status: status_code,
            stdout: stdout_buf,
            stderr: stderr_buf,
        })
    }
}
