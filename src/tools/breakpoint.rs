use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;

use super::DebugServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetBreakpointParams {
    /// Source file path.
    pub file: String,
    /// Line number (1-indexed).
    pub line: i64,
    /// Optional condition expression.
    pub condition: Option<String>,
}

impl DebugServer {
    /// Set a breakpoint at a specific file and line.
    pub async fn handle_set_breakpoint(
        &self,
        params: SetBreakpointParams,
    ) -> Result<CallToolResult, McpError> {
        // TODO: send DAP setBreakpoints request
        let _ = params;
        Ok(CallToolResult::success(vec![Content::text(
            "debug_set_breakpoint: not yet implemented",
        )]))
    }
}
