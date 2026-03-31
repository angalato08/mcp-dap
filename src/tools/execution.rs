use std::fmt::Write;
use std::time::Duration;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;

use super::DebugServer;
use crate::context::sanitize::sanitize_debuggee_output;
use crate::dap::state_machine::SessionPhase;
use crate::dap::types::DapEvent;
use crate::error::AppError;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContinueParams {
    /// Thread ID to continue. Uses first available thread if omitted.
    pub thread_id: Option<i64>,
    /// If true, only resume this thread; other threads stay paused.
    #[serde(default)]
    pub single_thread: bool,
    /// Timeout in seconds to wait for the next stop event. Defaults to config value.
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StepParams {
    /// Step granularity: "in", "out", or "over".
    pub granularity: StepGranularity,
    /// Thread ID to step. Uses first available thread if omitted.
    pub thread_id: Option<i64>,
    /// If true, only step this thread; other threads stay paused.
    #[serde(default)]
    pub single_thread: bool,
    /// Timeout in seconds to wait for the next stop event. Defaults to config value.
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StepGranularity {
    In,
    Out,
    Over,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PauseParams {
    /// Thread ID to pause. If omitted, pauses all threads.
    pub thread_id: Option<i64>,
    /// Timeout in seconds to wait for the stop event confirming pause. Defaults to config value.
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThreadsParams {}

impl DebugServer {
    /// Handle a stopped/exited/terminated event with auto-context enrichment.
    async fn handle_stopped_event(
        &self,
        event: &DapEvent,
        action_label: &str,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;

        match event {
            DapEvent::Stopped {
                thread_id,
                reason,
                all_threads_stopped,
            } => {
                state
                    .session
                    .lock()
                    .await
                    .transition(SessionPhase::Stopped)
                    .map_err(McpError::from)?;
                let scope = if *all_threads_stopped {
                    ", all threads stopped"
                } else {
                    ""
                };
                let header = format!("{action_label}: {reason} (thread {thread_id}{scope})");
                let rich = self.build_stopped_auto_context(*thread_id, &header).await;
                Ok(CallToolResult::success(vec![Content::text(rich)]))
            }
            DapEvent::Exited { exit_code } => {
                state
                    .session
                    .lock()
                    .await
                    .transition(SessionPhase::Terminated)
                    .map_err(McpError::from)?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Process exited with code {exit_code}"
                ))]))
            }
            DapEvent::Terminated => {
                state
                    .session
                    .lock()
                    .await
                    .transition(SessionPhase::Terminated)
                    .map_err(McpError::from)?;
                Ok(CallToolResult::success(vec![Content::text(
                    "Debug session terminated".to_string(),
                )]))
            }
            _ => unreachable!(),
        }
    }

    /// Resolve a thread ID: use the provided one, or query the adapter for the first thread.
    pub(super) async fn resolve_thread_id(&self, thread_id: Option<i64>) -> Result<i64, AppError> {
        if let Some(id) = thread_id {
            return Ok(id);
        }

        let timeout = self.state.config.dap_timeout_secs;
        let guard = self.state.require_client().await?;
        let client = guard.as_ref().unwrap();
        let body = client
            .send_request_with_timeout("threads", None, timeout)
            .await?;

        let thread_id = body
            .get("threads")
            .and_then(serde_json::Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|t| t.get("id"))
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| AppError::DapError("no threads available".into()))?;

        Ok(thread_id)
    }

    /// Resume execution until next breakpoint or exit.
    pub async fn handle_continue(
        &self,
        params: ContinueParams,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;

        // Resolve thread before sending continue.
        let thread_id = self
            .resolve_thread_id(params.thread_id)
            .await
            .map_err(McpError::from)?;

        // Subscribe before sending the request so we don't miss the event.
        let mut event_rx = state.event_tx.subscribe();
        let timeout = params.timeout.unwrap_or(state.config.dap_timeout_secs);

        // Send DAP continue.
        {
            let mut args = serde_json::json!({ "threadId": thread_id });
            if params.single_thread {
                args["singleThread"] = serde_json::json!(true);
            }

            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout("continue", Some(args), timeout)
                .await
                .map_err(McpError::from)?;
        }

        // Transition Stopped -> Running.
        state
            .session
            .lock()
            .await
            .transition(SessionPhase::Running)
            .map_err(McpError::from)?;

        // Wait for stop/terminate/exit event.
        let event = tokio::time::timeout(Duration::from_secs(timeout), async {
            loop {
                match event_rx.recv().await {
                    Ok(
                        ev @ (DapEvent::Stopped { .. }
                        | DapEvent::Terminated
                        | DapEvent::Exited { .. }),
                    ) => return Ok(ev),
                    Ok(_) => {}
                    Err(e) => {
                        return Err(AppError::DapError(format!("event channel error: {e}")));
                    }
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
        .map_err(McpError::from)?;

        self.handle_stopped_event(&event, "Stopped").await
    }

    /// Step in, out, or over the current line.
    pub async fn handle_step(&self, params: StepParams) -> Result<CallToolResult, McpError> {
        let state = &self.state;

        let thread_id = self
            .resolve_thread_id(params.thread_id)
            .await
            .map_err(McpError::from)?;

        // Map granularity to DAP command.
        let command = match params.granularity {
            StepGranularity::Over => "next",
            StepGranularity::In => "stepIn",
            StepGranularity::Out => "stepOut",
        };

        // Subscribe before sending.
        let mut event_rx = state.event_tx.subscribe();
        let timeout = params.timeout.unwrap_or(state.config.dap_timeout_secs);

        // Send DAP step request.
        {
            let mut args = serde_json::json!({ "threadId": thread_id });
            if params.single_thread {
                args["singleThread"] = serde_json::json!(true);
            }

            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(command, Some(args), timeout)
                .await
                .map_err(McpError::from)?;
        }

        // Transition Stopped -> Running.
        state
            .session
            .lock()
            .await
            .transition(SessionPhase::Running)
            .map_err(McpError::from)?;

        // Wait for stopped event.
        let event = tokio::time::timeout(Duration::from_secs(timeout), async {
            loop {
                match event_rx.recv().await {
                    Ok(
                        ev @ (DapEvent::Stopped { .. }
                        | DapEvent::Terminated
                        | DapEvent::Exited { .. }),
                    ) => return Ok(ev),
                    Ok(_) => {}
                    Err(e) => {
                        return Err(AppError::DapError(format!("event channel error: {e}")));
                    }
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
        .map_err(McpError::from)?;

        self.handle_stopped_event(&event, &format!("Stepped ({command})"))
            .await
    }

    /// Pause one or all threads.
    pub async fn handle_pause(&self, params: PauseParams) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = params.timeout.unwrap_or(state.config.dap_timeout_secs);

        let thread_id = self
            .resolve_thread_id(params.thread_id)
            .await
            .map_err(McpError::from)?;

        {
            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "pause",
                    Some(serde_json::json!({ "threadId": thread_id })),
                    timeout,
                )
                .await
                .map_err(McpError::from)?;
        }

        // Wait for the stopped event confirming the pause.
        let mut event_rx = state.event_tx.subscribe();
        let event = tokio::time::timeout(Duration::from_secs(timeout), async {
            loop {
                match event_rx.recv().await {
                    Ok(ev @ DapEvent::Stopped { .. }) => return Ok(ev),
                    Ok(DapEvent::Terminated | DapEvent::AdapterCrashed) => {
                        return Err(AppError::DapError("session ended during pause".into()));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        return Err(AppError::DapError(format!("event channel error: {e}")));
                    }
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
        .map_err(McpError::from)?;

        self.handle_stopped_event(&event, "Paused").await
    }

    /// List all threads in the debuggee.
    pub async fn handle_threads(&self) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        let body = {
            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout("threads", None, timeout)
                .await
                .map_err(McpError::from)?
        };

        let threads = body
            .get("threads")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        if threads.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No threads available",
            )]));
        }

        let mut output = String::new();
        for t in &threads {
            let id = t
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(-1);
            let name = sanitize_debuggee_output(
                t.get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unnamed"),
            );
            let _ = writeln!(output, "Thread {id}: {name}");
        }
        let _ = write!(output, "\n{} thread(s) total", threads.len());

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
}
