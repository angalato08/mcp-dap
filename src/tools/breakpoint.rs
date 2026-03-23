use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::DebugServer;

/// A tracked breakpoint with line and optional condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedBreakpoint {
    pub line: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetBreakpointParams {
    /// Source file path.
    pub file: String,
    /// Line number (1-indexed).
    pub line: i64,
    /// Optional condition expression.
    pub condition: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveBreakpointParams {
    /// Source file path.
    pub file: String,
    /// Line number (1-indexed).
    pub line: i64,
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
        let bp = TrackedBreakpoint {
            line: params.line,
            condition: params.condition.clone(),
        };

        // Add to the per-file breakpoint tracker.
        {
            let mut bps = state.breakpoints.lock().await;
            bps.entry(params.file.clone()).or_default().push(bp);
        }

        // Send the full list for this file.
        let breakpoints = state.breakpoints.lock().await;
        let file_bps = breakpoints.get(&params.file).unwrap();
        let dap_bps: Vec<serde_json::Value> = file_bps
            .iter()
            .map(|b| {
                let mut v = serde_json::json!({ "line": b.line });
                if let Some(cond) = &b.condition {
                    v["condition"] = serde_json::json!(cond);
                }
                v
            })
            .collect();

        let body = {
            let guard = state.require_client().await.map_err(McpError::from)?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "setBreakpoints",
                    Some(serde_json::json!({
                        "source": { "path": params.file },
                        "breakpoints": dap_bps,
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
            .rfind(|b| b.get("line").and_then(serde_json::Value::as_i64) == Some(params.line));

        let msg = if let Some(bp) = our_bp {
            let verified = bp.get("verified").and_then(serde_json::Value::as_bool).unwrap_or(false);
            let actual_line = bp.get("line").and_then(serde_json::Value::as_i64).unwrap_or(params.line);
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

    /// Remove a breakpoint at a specific file and line.
    pub async fn handle_remove_breakpoint(
        &self,
        params: RemoveBreakpointParams,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;

        let remaining = {
            let mut bps = state.breakpoints.lock().await;
            let Some(file_bps) = bps.get_mut(&params.file) else {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "No breakpoint at {}:{}", params.file, params.line
                ))]));
            };

            if !file_bps.iter().any(|b| b.line == params.line) {
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "No breakpoint at {}:{}", params.file, params.line
                ))]));
            }

            file_bps.retain(|b| b.line != params.line);
            let remaining = file_bps.len();

            // Send updated list to DAP.
            let dap_bps: Vec<serde_json::Value> = file_bps
                .iter()
                .map(|b| {
                    let mut v = serde_json::json!({ "line": b.line });
                    if let Some(cond) = &b.condition {
                        v["condition"] = serde_json::json!(cond);
                    }
                    v
                })
                .collect();

            {
                let guard = state.require_client().await.map_err(McpError::from)?;
                let client = guard.as_ref().unwrap();
                client
                    .send_request_with_timeout(
                        "setBreakpoints",
                        Some(serde_json::json!({
                            "source": { "path": params.file },
                            "breakpoints": dap_bps,
                        })),
                        timeout,
                    )
                    .await
                    .map_err(McpError::from)?;
            }

            if file_bps.is_empty() {
                bps.remove(&params.file);
            }

            remaining
        };

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Breakpoint removed at {}:{} ({remaining} remaining)",
            params.file, params.line
        ))]))
    }
}
