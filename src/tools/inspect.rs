use std::path::Path;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::warn;

use super::DebugServer;
use crate::context::source::extract_source_context;
use crate::context::truncation::truncate_value;
use crate::error::AppError;

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
    /// Resolve the top frame ID for evaluate context when none is provided.
    async fn resolve_frame_id(&self, frame_id: Option<i64>) -> Result<i64, AppError> {
        if let Some(id) = frame_id {
            return Ok(id);
        }

        // Get the first thread, then its top stack frame.
        let thread_id = self.resolve_thread_id(None).await?;
        let timeout = self.state.config.dap_timeout_secs;

        let guard = self.state.require_client().await?;
        let client = guard.as_ref().unwrap();
        let body = client
            .send_request_with_timeout(
                "stackTrace",
                Some(serde_json::json!({
                    "threadId": thread_id,
                    "levels": 1,
                })),
                timeout,
            )
            .await?;

        let frame_id = body
            .get("stackFrames")
            .and_then(|f| f.as_array())
            .and_then(|arr| arr.first())
            .and_then(|f| f.get("id"))
            .and_then(|id| id.as_i64())
            .ok_or_else(|| AppError::DapError("no stack frames available".into()))?;

        Ok(frame_id)
    }

    /// Get the current call stack with auto-injected source context.
    pub async fn handle_get_stack(
        &self,
        params: GetStackParams,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;
        let context_lines = state.config.source_context_lines;

        let thread_id = self
            .resolve_thread_id(params.thread_id)
            .await
            .map_err(McpError::from)?;

        let body = {
            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "stackTrace",
                    Some(serde_json::json!({
                        "threadId": thread_id,
                        "levels": params.max_frames,
                    })),
                    timeout,
                )
                .await
                .map_err(McpError::from)?
        };

        let frames = body
            .get("stackFrames")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();

        if frames.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No stack frames available",
            )]));
        }

        let mut output = String::new();
        // Inject source context for the top 3 frames.
        const SOURCE_INJECT_FRAMES: usize = 3;

        for (i, frame) in frames.iter().enumerate() {
            let name = frame
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("<unknown>");
            let file = frame
                .get("source")
                .and_then(|s| s.get("path"))
                .and_then(|p| p.as_str());
            let line = frame.get("line").and_then(|l| l.as_i64());

            // Format: #0 function_name at file:line
            output.push_str(&format!("#{i} {name}"));
            if let (Some(file), Some(line)) = (file, line) {
                output.push_str(&format!(" at {file}:{line}"));
            }
            output.push('\n');

            // Auto-inject source context for top frames.
            if i < SOURCE_INJECT_FRAMES {
                if let (Some(file), Some(line)) = (file, line) {
                    let path = Path::new(file);
                    match extract_source_context(path, line as usize, context_lines) {
                        Ok(snippet) => {
                            output.push_str(&snippet);
                            output.push('\n');
                        }
                        Err(e) => {
                            warn!(file, "failed to read source for context: {e}");
                        }
                    }
                }
            }
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Evaluate an expression or read a variable, with LLM context guard.
    pub async fn handle_evaluate(
        &self,
        params: EvaluateParams,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        let frame_id = self
            .resolve_frame_id(params.frame_id)
            .await
            .map_err(McpError::from)?;

        let body = {
            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "evaluate",
                    Some(serde_json::json!({
                        "expression": params.expression,
                        "frameId": frame_id,
                        "context": "repl",
                    })),
                    timeout,
                )
                .await
                .map_err(McpError::from)?
        };

        let result_str = body
            .get("result")
            .and_then(|r| r.as_str())
            .unwrap_or("<no result>");

        let type_name = body
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");

        // Apply LLM context guard truncation.
        let truncated = truncate_value(result_str, state.config.max_variable_length);

        Ok(CallToolResult::success(vec![Content::text(format!(
            "{} = {} (type: {type_name})",
            params.expression, truncated,
        ))]))
    }
}
