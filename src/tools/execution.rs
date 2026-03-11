use std::time::Duration;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;

use super::DebugServer;
use crate::dap::state_machine::SessionPhase;
use crate::dap::types::DapEvent;
use crate::error::AppError;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContinueParams {
    /// Thread ID to continue. Uses first available thread if omitted.
    pub thread_id: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StepParams {
    /// Step granularity: "in", "out", or "over".
    pub granularity: StepGranularity,
    /// Thread ID to step. Uses first available thread if omitted.
    pub thread_id: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StepGranularity {
    In,
    Out,
    Over,
}

impl DebugServer {
    /// Resolve a thread ID: use the provided one, or query the adapter for the first thread.
    async fn resolve_thread_id(&self, thread_id: Option<i64>) -> Result<i64, AppError> {
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
            .and_then(|t| t.as_array())
            .and_then(|arr| arr.first())
            .and_then(|t| t.get("id"))
            .and_then(|id| id.as_i64())
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
        let timeout = state.config.dap_timeout_secs;

        // Send DAP continue.
        {
            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "continue",
                    Some(serde_json::json!({ "threadId": thread_id })),
                    timeout,
                )
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
                    Ok(_) => continue,
                    Err(e) => {
                        return Err(AppError::DapError(format!("event channel error: {e}")))
                    }
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
        .map_err(McpError::from)?;

        // Update session state and build response.
        match &event {
            DapEvent::Stopped { thread_id, reason } => {
                state
                    .session
                    .lock()
                    .await
                    .transition(SessionPhase::Stopped)
                    .map_err(McpError::from)?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Stopped: {reason} (thread {thread_id})"
                ))]))
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
        let timeout = state.config.dap_timeout_secs;

        // Send DAP step request.
        {
            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    command,
                    Some(serde_json::json!({ "threadId": thread_id })),
                    timeout,
                )
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
                    Ok(_) => continue,
                    Err(e) => {
                        return Err(AppError::DapError(format!("event channel error: {e}")))
                    }
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
        .map_err(McpError::from)?;

        match &event {
            DapEvent::Stopped { thread_id, reason } => {
                state
                    .session
                    .lock()
                    .await
                    .transition(SessionPhase::Stopped)
                    .map_err(McpError::from)?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Stepped ({command}): {reason} (thread {thread_id})"
                ))]))
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
}
