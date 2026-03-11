use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;

use super::DebugServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetStackParams {
    /// Thread ID. Uses first available thread if omitted.
    pub thread_id: Option<i64>,
    /// Maximum number of frames to return.
    #[serde(default = "default_max_frames")]
    pub max_frames: usize,
}

fn default_max_frames() -> usize {
    20
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvaluateParams {
    /// Expression to evaluate in the current frame.
    pub expression: String,
    /// Frame ID for evaluation context. Uses top frame if omitted.
    pub frame_id: Option<i64>,
}

impl DebugServer {
    /// Get the current call stack with auto-injected source context.
    pub async fn handle_get_stack(
        &self,
        params: GetStackParams,
    ) -> Result<CallToolResult, McpError> {
        // TODO: DAP stackTrace, read source files, format with context
        let _ = params;
        Ok(CallToolResult::success(vec![Content::text(
            "debug_get_stack: not yet implemented",
        )]))
    }

    /// Evaluate an expression or read a variable, with LLM context guard.
    pub async fn handle_evaluate(
        &self,
        params: EvaluateParams,
    ) -> Result<CallToolResult, McpError> {
        // TODO: DAP evaluate, pipe through truncation/summarization
        let _ = params;
        Ok(CallToolResult::success(vec![Content::text(
            "debug_evaluate: not yet implemented",
        )]))
    }
}
