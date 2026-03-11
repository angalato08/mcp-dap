use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;

use super::DebugServer;

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
    /// Launch a debug session: spawn adapter, initialize, and launch target.
    pub async fn handle_launch(&self, params: LaunchParams) -> Result<CallToolResult, McpError> {
        // TODO: spawn adapter, DAP initialize handshake, launch request, start event loop
        let _ = params;
        Ok(CallToolResult::success(vec![Content::text(
            "debug_launch: not yet implemented",
        )]))
    }

    /// Attach to an already-running process.
    pub async fn handle_attach(&self, params: AttachParams) -> Result<CallToolResult, McpError> {
        // TODO: spawn adapter, DAP initialize handshake, attach request
        let _ = params;
        Ok(CallToolResult::success(vec![Content::text(
            "debug_attach: not yet implemented",
        )]))
    }
}
