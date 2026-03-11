use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;

use super::DebugServer;

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
    /// Resume execution until next breakpoint or exit.
    pub async fn handle_continue(
        &self,
        params: ContinueParams,
    ) -> Result<CallToolResult, McpError> {
        // TODO: send DAP continue, await stopped/terminated event
        let _ = params;
        Ok(CallToolResult::success(vec![Content::text(
            "debug_continue: not yet implemented",
        )]))
    }

    /// Step in, out, or over the current line.
    pub async fn handle_step(&self, params: StepParams) -> Result<CallToolResult, McpError> {
        // TODO: send DAP stepIn/stepOut/next based on granularity
        let _ = params;
        Ok(CallToolResult::success(vec![Content::text(
            "debug_step: not yet implemented",
        )]))
    }
}
