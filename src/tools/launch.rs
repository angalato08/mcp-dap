use std::time::Duration;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{debug, info};

use super::DebugServer;
use crate::dap::client::DapClient;
use crate::dap::state_machine::SessionPhase;
use crate::dap::transport::spawn_adapter;
use crate::dap::types::DapEvent;
use crate::error::AppError;
use crate::services::event_loop::run_event_loop;

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
    pub async fn handle_launch(&self, params: LaunchParams) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        // Guard: no active session.
        state.require_no_client().await.map_err(McpError::from)?;

        // Transition: Uninitialized -> Initializing.
        state
            .session
            .lock()
            .await
            .transition(SessionPhase::Initializing)
            .map_err(McpError::from)?;

        // Spawn the debug adapter subprocess.
        let process =
            spawn_adapter(&params.adapter_path, &params.adapter_args).map_err(McpError::from)?;
        info!(adapter = %params.adapter_path, "debug adapter spawned");

        let client = DapClient::new(process);

        // Take the reader and start the event loop in a background task.
        let reader = client
            .reader
            .lock()
            .await
            .take()
            .expect("reader should be available on new client");
        let pending = client.pending.clone();
        let event_tx = state.event_tx.clone();
        tokio::spawn(async move {
            run_event_loop(reader, pending, event_tx).await;
        });

        // Store client in shared state.
        *state.dap_client.lock().await = Some(client);

        // Subscribe to events before sending initialize (so we don't miss `initialized`).
        let mut event_rx = state.event_tx.subscribe();

        // DAP: initialize
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().unwrap();
            let caps = client
                .send_request_with_timeout(
                    "initialize",
                    Some(serde_json::json!({
                        "clientID": "mcp-dap-rs",
                        "clientName": "mcp-dap-rs",
                        "adapterID": "mcp-dap-rs",
                        "linesStartAt1": true,
                        "columnsStartAt1": true,
                        "pathFormat": "path",
                    })),
                    timeout,
                )
                .await
                .map_err(McpError::from)?;
            debug!(?caps, "adapter capabilities");
        }

        // Wait for the `initialized` event from the adapter.
        tokio::time::timeout(Duration::from_secs(timeout), async {
            loop {
                match event_rx.recv().await {
                    Ok(DapEvent::Initialized) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?;

        // DAP: configurationDone
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout("configurationDone", None, timeout)
                .await
                .map_err(McpError::from)?;
        }

        // DAP: launch
        {
            let mut launch_args = serde_json::json!({
                "program": params.program,
                "args": params.program_args,
                "stopOnEntry": params.stop_on_entry,
            });
            if let Some(cwd) = &params.cwd {
                launch_args["cwd"] = serde_json::json!(cwd);
            }

            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout("launch", Some(launch_args), timeout)
                .await
                .map_err(McpError::from)?;
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
                        Ok(DapEvent::Stopped { .. }) => break,
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
            })
            .await
            .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?;

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
    pub async fn handle_attach(&self, params: AttachParams) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        state.require_no_client().await.map_err(McpError::from)?;

        state
            .session
            .lock()
            .await
            .transition(SessionPhase::Initializing)
            .map_err(McpError::from)?;

        let process =
            spawn_adapter(&params.adapter_path, &params.adapter_args).map_err(McpError::from)?;
        info!(adapter = %params.adapter_path, "debug adapter spawned for attach");

        let client = DapClient::new(process);

        let reader = client
            .reader
            .lock()
            .await
            .take()
            .expect("reader should be available on new client");
        let pending = client.pending.clone();
        let event_tx = state.event_tx.clone();
        tokio::spawn(async move {
            run_event_loop(reader, pending, event_tx).await;
        });

        *state.dap_client.lock().await = Some(client);

        let mut event_rx = state.event_tx.subscribe();

        // DAP: initialize
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "initialize",
                    Some(serde_json::json!({
                        "clientID": "mcp-dap-rs",
                        "clientName": "mcp-dap-rs",
                        "adapterID": "mcp-dap-rs",
                        "linesStartAt1": true,
                        "columnsStartAt1": true,
                        "pathFormat": "path",
                    })),
                    timeout,
                )
                .await
                .map_err(McpError::from)?;
        }

        // Wait for initialized event.
        tokio::time::timeout(Duration::from_secs(timeout), async {
            loop {
                match event_rx.recv().await {
                    Ok(DapEvent::Initialized) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        })
        .await
        .map_err(|_| McpError::from(AppError::DapTimeout(timeout)))?;

        // DAP: configurationDone
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout("configurationDone", None, timeout)
                .await
                .map_err(McpError::from)?;
        }

        // DAP: attach
        {
            let guard = state.dap_client.lock().await;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "attach",
                    Some(serde_json::json!({ "pid": params.pid })),
                    timeout,
                )
                .await
                .map_err(McpError::from)?;
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
    pub async fn handle_disconnect(&self) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        // Try to send disconnect gracefully.
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

        // Kill the child process.
        {
            let guard = state.dap_client.lock().await;
            if let Some(client) = guard.as_ref() {
                let _ = client.child.lock().await.kill().await;
            }
        }

        // Clear the client.
        *state.dap_client.lock().await = None;

        // Reset session state to Uninitialized.
        // Force-reset by creating a new SessionState since we may be in any phase.
        *state.session.lock().await = crate::dap::state_machine::SessionState::new();

        // Clear breakpoint tracker.
        state.breakpoints.lock().await.clear();

        info!("debug session disconnected");

        Ok(CallToolResult::success(vec![Content::text(
            "Debug session disconnected",
        )]))
    }
}
