use std::time::Duration;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument};

use tokio::sync::broadcast;

use super::DebugServer;
use crate::dap::client::DapClient;
use crate::dap::state_machine::SessionPhase;
use crate::dap::transport::{spawn_adapter, spawn_tcp_adapter};
use crate::dap::types::DapEvent;
use crate::error::AppError;
use crate::services::event_loop::run_event_loop;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(untagged)]
pub enum AdapterTransport {
    /// Communicate over adapter's stdin/stdout (default).
    #[default]
    Stdio,
    /// Communicate over TCP; adapter listens on this port.
    Tcp(u16),
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LaunchParams {
    /// Path to the debug adapter executable (e.g. "codelldb", "debugpy").
    pub adapter_path: String,
    /// Arguments to pass to the debug adapter.
    #[serde(default)]
    pub adapter_args: Vec<String>,
    /// Program to debug.
    pub program: String,
    /// Arguments to pass to the debugged program.
    #[serde(default)]
    pub program_args: Vec<String>,
    /// Working directory for the debugged program.
    pub cwd: Option<String>,
    /// Stop at entry point.
    #[serde(default)]
    pub stop_on_entry: bool,
    /// Transport mode: "stdio" (default) or {"tcp": port}.
    #[serde(default)]
    pub transport: AdapterTransport,
    /// Extra adapter-specific launch arguments (merged into DAP launch request).
    #[serde(default)]
    pub extra_launch_args: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AttachParams {
    /// Path to the debug adapter executable.
    pub adapter_path: String,
    /// Arguments to pass to the debug adapter.
    #[serde(default)]
    pub adapter_args: Vec<String>,
    /// Process ID to attach to.
    pub pid: i64,
}

impl DebugServer {
    /// Spawn adapter, run DAP initialize/configurationDone/launch handshake, start event loop.
    #[instrument(skip(self, params), fields(adapter = %params.adapter_path))]
    pub async fn handle_launch(&self, params: LaunchParams) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        // Validate adapter path against whitelist.
        state
            .validate_adapter_path(&params.adapter_path)
            .map_err(McpError::from)?;

        // Atomic guard: no active session + transition to Initializing.
        state.try_start_session().await.map_err(McpError::from)?;

        // Spawn the debug adapter subprocess.
        let client = match &params.transport {
            AdapterTransport::Stdio => {
                let process = match spawn_adapter(&params.adapter_path, &params.adapter_args) {
                    Ok(p) => p,
                    Err(e) => {
                        state.force_cleanup().await;
                        return Err(McpError::from(e));
                    }
                };
                DapClient::new(process)
            }
            AdapterTransport::Tcp(port) => {
                match spawn_tcp_adapter(&params.adapter_path, &params.adapter_args, *port).await {
                    Ok(c) => c,
                    Err(e) => {
                        state.force_cleanup().await;
                        return Err(McpError::from(e));
                    }
                }
            }
        };
        info!(adapter = %params.adapter_path, "debug adapter spawned");

        // Subscribe BEFORE spawning the event loop so we don't miss early events.
        let event_rx = state.event_tx.subscribe();

        // Take the reader and start the event loop in a background task.
        let reader = client
            .reader
            .lock()
            .await
            .take()
            .expect("reader should be available on new client");
        let pending = client.pending.clone();
        let writer = client.writer_handle();
        let event_tx = state.event_tx.clone();
        let crash_state = state.clone();
        tokio::spawn(async move {
            run_event_loop(reader, pending, event_tx, writer).await;
            // Adapter disconnected — clean up if session is still active.
            crash_state.force_cleanup().await;
            tracing::warn!("adapter process exited, session state cleaned up");
        });

        // Store client in shared state.
        *state.dap_client.lock().await = Some(client);

        // Run the DAP handshake, cleaning up on any failure.
        match self
            .launch_handshake(state, timeout, &params, event_rx)
            .await
        {
            Ok(result) => Ok(result),
            Err(e) => {
                state.force_cleanup().await;
                Err(e)
            }
        }
    }

    /// DAP handshake sequence: initialize → launch → initialized event → configurationDone.
    /// Separated from `handle_launch` so we can clean up on any failure.
    #[allow(clippy::too_many_lines)]
    async fn launch_handshake(
        &self,
        state: &crate::state::AppState,
        timeout: u64,
        params: &LaunchParams,
        mut event_rx: broadcast::Receiver<DapEvent>,
    ) -> Result<CallToolResult, McpError> {
        // DAP: initialize
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().ok_or(McpError::from(AppError::NoSession))?;
            let caps = client
                .send_initialize(timeout)
                .await
                .map_err(McpError::from)?;
            debug!(?caps, "adapter capabilities");
            *state.capabilities.lock().await = Some(caps);
        }

        // DAP: launch — send WITHOUT blocking on the response.
        // Some adapters (debugpy) delay the launch response until after configurationDone,
        // so we must not block here. The oneshot channel buffers it for later collection.
        let launch_rx = {
            let mut launch_args = serde_json::json!({
                "program": params.program,
                "args": params.program_args,
                "stopOnEntry": params.stop_on_entry,
                "console": "internalConsole",
            });
            if let Some(cwd) = &params.cwd {
                launch_args["cwd"] = serde_json::json!(cwd);
            }
            // Merge adapter-specific extra arguments.
            if let Some(extra) = &params.extra_launch_args
                && let (Some(base), Some(extra)) = (launch_args.as_object_mut(), extra.as_object())
            {
                for (k, v) in extra {
                    base.insert(k.clone(), v.clone());
                }
            }

            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().ok_or(McpError::from(AppError::NoSession))?;
            client
                .send_request("launch", Some(launch_args))
                .await
                .map_err(McpError::from)?
        };

        // Wait for the `initialized` event from the adapter.
        tokio::time::timeout(Duration::from_secs(timeout), async {
            loop {
                match event_rx.recv().await {
                    Ok(DapEvent::Initialized) => return Ok(()),
                    Ok(DapEvent::Exited { exit_code }) => {
                        return Err(AppError::DapError(format!(
                            "debuggee exited (code {exit_code}) before adapter initialized"
                        )));
                    }
                    Ok(DapEvent::Terminated | DapEvent::AdapterCrashed) => {
                        return Err(AppError::DapError(
                            "adapter terminated before initialization completed".into(),
                        ));
                    }
                    Err(_) => {
                        return Err(AppError::DapError(
                            "event channel closed during initialization".into(),
                        ));
                    }
                    Ok(_) => {}
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
        .map_err(McpError::from)?;

        // DAP: configurationDone (after initialized event, per DAP spec).
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().ok_or(McpError::from(AppError::NoSession))?;
            client
                .send_request_with_timeout("configurationDone", None, timeout)
                .await
                .map_err(McpError::from)?;
        }

        // Collect the deferred launch response (already buffered for fast adapters,
        // arrives after configurationDone for debugpy).
        {
            let response = tokio::time::timeout(Duration::from_secs(timeout), launch_rx)
                .await
                .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
                .map_err(|_| {
                    McpError::from(AppError::DapError("launch response channel closed".into()))
                })?;
            let success = response
                .get("success")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if !success {
                let message = response
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown DAP error");
                return Err(McpError::from(AppError::DapError(message.to_string())));
            }
        }

        // Transition: Initializing -> Running.
        state
            .session
            .lock()
            .await
            .transition(SessionPhase::Running)
            .map_err(McpError::from)?;

        info!(program = %params.program, "debug session launched");

        // If stopOnEntry, wait for the stopped event and transition to Stopped.
        if params.stop_on_entry {
            tokio::time::timeout(Duration::from_secs(timeout), async {
                loop {
                    match event_rx.recv().await {
                        Ok(DapEvent::Stopped { .. }) => return Ok(()),
                        Ok(DapEvent::Exited { exit_code }) => {
                            return Err(AppError::DapError(format!(
                                "debuggee exited (code {exit_code}) before stopping at entry"
                            )));
                        }
                        Ok(DapEvent::Terminated | DapEvent::AdapterCrashed) => {
                            return Err(AppError::DapError(
                                "debuggee terminated before stopping at entry".into(),
                            ));
                        }
                        Err(_) => {
                            return Err(AppError::DapError(
                                "event channel closed while waiting for entry stop".into(),
                            ));
                        }
                        Ok(_) => {}
                    }
                }
            })
            .await
            .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
            .map_err(McpError::from)?;

            state
                .session
                .lock()
                .await
                .transition(SessionPhase::Stopped)
                .map_err(McpError::from)?;
        }

        let status = if params.stop_on_entry {
            "stopped at entry"
        } else {
            "running"
        };

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Debug session started ({status}). Program: {}",
            params.program,
        ))]))
    }

    /// Attach to an already-running process.
    #[instrument(skip(self, params), fields(adapter = %params.adapter_path))]
    pub async fn handle_attach(&self, params: AttachParams) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        // Validate adapter path against whitelist.
        state
            .validate_adapter_path(&params.adapter_path)
            .map_err(McpError::from)?;

        // Atomic guard: no active session + transition to Initializing.
        state.try_start_session().await.map_err(McpError::from)?;

        let process = match spawn_adapter(&params.adapter_path, &params.adapter_args) {
            Ok(p) => p,
            Err(e) => {
                state.force_cleanup().await;
                return Err(McpError::from(e));
            }
        };
        info!(adapter = %params.adapter_path, "debug adapter spawned for attach");

        let client = DapClient::new(process);

        // Subscribe BEFORE spawning the event loop so we don't miss early events.
        let event_rx = state.event_tx.subscribe();

        let reader = client
            .reader
            .lock()
            .await
            .take()
            .expect("reader should be available on new client");
        let pending = client.pending.clone();
        let writer = client.writer_handle();
        let event_tx = state.event_tx.clone();
        let crash_state = state.clone();
        tokio::spawn(async move {
            run_event_loop(reader, pending, event_tx, writer).await;
            crash_state.force_cleanup().await;
            tracing::warn!("adapter process exited, session state cleaned up");
        });

        *state.dap_client.lock().await = Some(client);

        // Run the attach handshake, cleaning up on any failure.
        match self
            .attach_handshake(state, timeout, &params, event_rx)
            .await
        {
            Ok(result) => Ok(result),
            Err(e) => {
                state.force_cleanup().await;
                Err(e)
            }
        }
    }

    /// DAP attach handshake: initialize → attach → initialized event → configurationDone.
    async fn attach_handshake(
        &self,
        state: &crate::state::AppState,
        timeout: u64,
        params: &AttachParams,
        mut event_rx: broadcast::Receiver<DapEvent>,
    ) -> Result<CallToolResult, McpError> {
        // DAP: initialize
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().ok_or(McpError::from(AppError::NoSession))?;
            let caps = client
                .send_initialize(timeout)
                .await
                .map_err(McpError::from)?;
            *state.capabilities.lock().await = Some(caps);
        }

        // DAP: attach — send WITHOUT blocking (same as launch: some adapters
        // delay the response until after configurationDone).
        let attach_rx = {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().ok_or(McpError::from(AppError::NoSession))?;
            client
                .send_request("attach", Some(serde_json::json!({ "pid": params.pid })))
                .await
                .map_err(McpError::from)?
        };

        // Wait for initialized event.
        tokio::time::timeout(Duration::from_secs(timeout), async {
            loop {
                match event_rx.recv().await {
                    Ok(DapEvent::Initialized) => return Ok(()),
                    Ok(DapEvent::Exited { exit_code }) => {
                        return Err(AppError::DapError(format!(
                            "debuggee exited (code {exit_code}) before adapter initialized"
                        )));
                    }
                    Ok(DapEvent::Terminated | DapEvent::AdapterCrashed) => {
                        return Err(AppError::DapError(
                            "adapter terminated before initialization completed".into(),
                        ));
                    }
                    Err(_) => {
                        return Err(AppError::DapError(
                            "event channel closed during initialization".into(),
                        ));
                    }
                    Ok(_) => {}
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
        .map_err(McpError::from)?;

        // DAP: configurationDone (after initialized event, per DAP spec).
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().ok_or(McpError::from(AppError::NoSession))?;
            client
                .send_request_with_timeout("configurationDone", None, timeout)
                .await
                .map_err(McpError::from)?;
        }

        // Collect the deferred attach response.
        {
            let response = tokio::time::timeout(Duration::from_secs(timeout), attach_rx)
                .await
                .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?
                .map_err(|_| {
                    McpError::from(AppError::DapError("attach response channel closed".into()))
                })?;
            let success = response
                .get("success")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if !success {
                let message = response
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown DAP error");
                return Err(McpError::from(AppError::DapError(message.to_string())));
            }
        }

        state
            .session
            .lock()
            .await
            .transition(SessionPhase::Running)
            .map_err(McpError::from)?;

        info!(pid = params.pid, "attached to process");

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Attached to process (PID: {})",
            params.pid,
        ))]))
    }

    /// Disconnect the debug session: send DAP disconnect, kill adapter, reset state.
    #[instrument(skip(self))]
    pub async fn handle_disconnect(&self) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        // Try to send disconnect gracefully (best-effort).
        {
            let guard = state.dap_client.lock().await;
            if let Some(client) = guard.as_ref() {
                let _ = client
                    .send_request_with_timeout(
                        "disconnect",
                        Some(serde_json::json!({ "terminateDebuggee": true })),
                        timeout,
                    )
                    .await;
            }
        }

        state.force_cleanup().await;

        info!("debug session disconnected");

        Ok(CallToolResult::success(vec![Content::text(
            "Debug session disconnected",
        )]))
    }
}
