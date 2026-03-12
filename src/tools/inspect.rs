use std::fmt::Write;
use std::path::Path;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::warn;

/// Number of top stack frames to inject source context for.
const SOURCE_INJECT_FRAMES: usize = 3;

use super::DebugServer;
use crate::context::sanitize::sanitize_debuggee_output;
use crate::context::source::extract_source_context;
use crate::context::truncation::{
    summarize_array, summarize_object, truncate_nested, truncate_value,
};
use crate::dap::state_machine::SessionPhase;
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
            .and_then(serde_json::Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|f| f.get("id"))
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| AppError::DapError("no stack frames available".into()))?;

        Ok(frame_id)
    }

    /// Fetch child variables and build a structured JSON summary.
    /// Returns `None` for scalar values (`variablesReference` <= 0).
    async fn fetch_children_structured(
        &self,
        variables_ref: i64,
        indexed_variables: Option<i64>,
        named_variables: Option<i64>,
    ) -> Result<Option<serde_json::Value>, AppError> {
        if variables_ref <= 0 {
            return Ok(None);
        }

        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;
        let max_var_len = state.config.max_variable_length;

        let body = {
            let guard = state.require_client().await?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "variables",
                    Some(serde_json::json!({ "variablesReference": variables_ref })),
                    timeout,
                )
                .await?
        };

        let children = body
            .get("variables")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        if children.is_empty() {
            return Ok(None);
        }

        // Determine array-like vs object-like.
        let is_array = match (indexed_variables, named_variables) {
            (Some(idx), Some(named)) if idx > 0 && named == 0 => true,
            (Some(_), Some(named)) if named > 0 => false,
            _ => {
                // Fallback: check if all child names parse as integers.
                children.iter().all(|c| {
                    c.get("name")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|n| n.parse::<usize>().is_ok())
                })
            }
        };

        let max_depth = state.config.max_nesting_depth;

        if is_array {
            let arr: Vec<serde_json::Value> = children
                .iter()
                .map(|c| {
                    let val = sanitize_debuggee_output(
                        c.get("value").and_then(serde_json::Value::as_str).unwrap_or(""),
                    );
                    let typ = c.get("type").and_then(serde_json::Value::as_str).unwrap_or("");
                    serde_json::json!({
                        "value": truncate_value(&val, max_var_len),
                        "type": typ,
                    })
                })
                .collect();
            let summarized = summarize_array(
                &serde_json::Value::Array(arr),
                state.config.max_array_items,
            );
            let summarized = truncate_nested(&summarized, max_depth);
            Ok(Some(summarized))
        } else {
            let mut obj = serde_json::Map::new();
            for c in &children {
                let name = sanitize_debuggee_output(
                    c.get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("?"),
                );
                let val = sanitize_debuggee_output(
                    c.get("value").and_then(serde_json::Value::as_str).unwrap_or(""),
                );
                let typ = c.get("type").and_then(serde_json::Value::as_str).unwrap_or("");
                obj.insert(
                    name,
                    serde_json::json!({
                        "value": truncate_value(&val, max_var_len),
                        "type": typ,
                    }),
                );
            }
            let summarized = summarize_object(
                &serde_json::Value::Object(obj),
                state.config.max_object_keys,
            );
            let summarized = truncate_nested(&summarized, max_depth);
            Ok(Some(summarized))
        }
    }

    /// Get the current call stack with auto-injected source context.
    pub async fn handle_get_stack(
        &self,
        params: GetStackParams,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        state
            .require_phase(&[SessionPhase::Running, SessionPhase::Stopped])
            .await
            .map_err(McpError::from)?;
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
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        if frames.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No stack frames available",
            )]));
        }

        let mut output = String::new();

        for (i, frame) in frames.iter().enumerate() {
            let name = sanitize_debuggee_output(
                frame
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("<unknown>"),
            );
            let file = frame
                .get("source")
                .and_then(|s| s.get("path"))
                .and_then(serde_json::Value::as_str);
            let line = frame.get("line").and_then(serde_json::Value::as_i64);

            // Format: #0 function_name at file:line
            let _ = write!(output, "#{i} {name}");
            if let (Some(file), Some(line)) = (file, line) {
                let _ = write!(output, " at {file}:{line}");
            }
            output.push('\n');

            // Auto-inject source context for top frames.
            if i < SOURCE_INJECT_FRAMES
                && let (Some(file), Some(line)) = (file, line)
            {
                let path = Path::new(file);
                match extract_source_context(path, usize::try_from(line).unwrap_or(0), context_lines) {
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

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
    /// Evaluate an expression or read a variable, with LLM context guard.
    pub async fn handle_evaluate(
        &self,
        params: EvaluateParams,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        state
            .require_phase(&[SessionPhase::Running, SessionPhase::Stopped])
            .await
            .map_err(McpError::from)?;
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
                        "context": "watch",
                    })),
                    timeout,
                )
                .await
                .map_err(McpError::from)?
        };

        let result_str = sanitize_debuggee_output(
            body.get("result")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<no result>"),
        );

        let type_name = body
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");

        let variables_ref = body
            .get("variablesReference")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        // For compound values, fetch children and return structured summary.
        if variables_ref > 0 {
            let indexed = body.get("indexedVariables").and_then(serde_json::Value::as_i64);
            let named = body.get("namedVariables").and_then(serde_json::Value::as_i64);

            match self
                .fetch_children_structured(variables_ref, indexed, named)
                .await
            {
                Ok(Some(structured)) => {
                    let pretty = serde_json::to_string_pretty(&structured)
                        .unwrap_or_else(|_| structured.to_string());
                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "{} (type: {type_name})\n{pretty}",
                        params.expression,
                    ))]));
                }
                Ok(None) => {} // No children, fall through to scalar path.
                Err(e) => {
                    warn!("failed to fetch children for {}: {e}", params.expression);
                    // Fall through to scalar path.
                }
            }
        }

        // Scalar value or fallback: truncate string representation.
        let truncated = truncate_value(&result_str, state.config.max_variable_length);
        Ok(CallToolResult::success(vec![Content::text(format!(
            "{} = {} (type: {type_name})",
            params.expression, truncated,
        ))]))
    }
}
