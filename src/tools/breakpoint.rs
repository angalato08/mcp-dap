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
    /// DAP's setBreakpoints replaces all breakpoints for a file, so we track them.
    pub async fn handle_set_breakpoint(
        &self,
        params: SetBreakpointParams,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        // Build the new breakpoint entry.
        let mut bp = serde_json::json!({ "line": params.line });
        if let Some(cond) = &params.condition {
            bp["condition"] = serde_json::json!(cond);
        }

        // Add to the per-file breakpoint tracker.
        {
            let mut bps = state.breakpoints.lock().await;
            bps.entry(params.file.clone()).or_default().push(bp);
        }

        // Send the full list for this file.
        let breakpoints = state.breakpoints.lock().await;
        let file_bps = breakpoints.get(&params.file).unwrap();

        let body = {
            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "setBreakpoints",
                    Some(serde_json::json!({
                        "source": { "path": params.file },
                        "breakpoints": file_bps,
                    })),
                    timeout,
                )
                .await
                .map_err(McpError::from)?
        };

        // Parse response to report back.
        let result_bps = body
            .get("breakpoints")
            .and_then(|b| b.as_array())
            .cloned()
            .unwrap_or_default();

        // Find the breakpoint we just set (last one matching our line).
        let our_bp = result_bps
            .iter()
            .rfind(|b| b.get("line").and_then(|l| l.as_i64()) == Some(params.line));

        let msg = if let Some(bp) = our_bp {
            let verified = bp.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
            let actual_line = bp.get("line").and_then(|l| l.as_i64()).unwrap_or(params.line);
            let status = if verified { "verified" } else { "pending" };
            let cond_str = params
                .condition
                .as_deref()
                .map(|c| format!(" (condition: {c})"))
                .unwrap_or_default();
            format!(
                "Breakpoint {status} at {}:{actual_line}{cond_str}",
                params.file,
            )
        } else {
            format!("Breakpoint set at {}:{}", params.file, params.line)
        };

        Ok(CallToolResult::success(vec![Content::text(msg)]))
    }
}
