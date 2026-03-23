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
use crate::context::pagination::CacheEntry;
use crate::context::truncation::{truncate_nested, truncate_value};
use crate::error::AppError;

use std::time::SystemTime;

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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPageParams {
    /// Pagination token from a previous truncated result.
    pub token: String,
    /// 0-based offset into the collection. Defaults to the next page after the initial result.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Maximum items to return. Defaults to config `max_array_items` / `max_object_keys`.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Pagination metadata for compact output formatting.
struct PaginationMeta<'a> {
    token: &'a str,
    total: usize,
    offset: usize,
    end: usize,
}

/// Format a compound JSON value (from `fetch_children_raw`) as compact text lines.
///
/// Arrays:  `[i] value (type)` per line
/// Objects: `key: value (type)` per line
fn format_compact(
    value: &serde_json::Value,
    is_array: bool,
    expression: &str,
    type_name: &str,
    pagination: Option<&PaginationMeta<'_>>,
) -> String {
    let mut out = String::new();

    // --- header line ---
    let _ = write!(out, "{expression} (");
    if is_array {
        let total = pagination.map_or_else(
            || value.as_array().map_or(0, Vec::len),
            |p| p.total,
        );
        let _ = write!(out, "{type_name}, {total} items");
        if let Some(p) = pagination {
            let _ = write!(out, ", showing {}..{}", p.offset, p.end);
        }
    } else {
        let total = pagination.map_or_else(
            || value.as_object().map_or(0, serde_json::Map::len),
            |p| p.total,
        );
        let _ = write!(out, "{type_name}, {total} keys");
        if let Some(p) = pagination {
            let _ = write!(out, ", showing {}..{}", p.offset, p.end);
        }
    }
    out.push(')');
    if let Some(p) = pagination {
        let _ = write!(out, " [page: {}]", p.token);
    }
    out.push('\n');

    // --- body lines ---
    if is_array {
        let items = value.as_array().map_or(&[][..], Vec::as_slice);
        let index_base = pagination.map_or(0, |p| p.offset);
        for (i, item) in items.iter().enumerate() {
            format_entry_line(&mut out, is_array, &(index_base + i).to_string(), item);
        }
    } else {
        let obj = value.as_object();
        if let Some(map) = obj {
            for (key, val) in map {
                if key.starts_with('_') && matches!(key.as_str(), "_summary" | "_pagination") {
                    continue;
                }
                format_entry_line(&mut out, is_array, key, val);
            }
        }
    }

    out
}

/// Format a single child entry line.
///
/// For `{"value": "...", "type": "..."}` objects (from `fetch_children_raw`),
/// extracts value/type. For depth-truncated strings, renders inline.
fn format_entry_line(out: &mut String, is_array: bool, key: &str, val: &serde_json::Value) {
    if is_array {
        let _ = write!(out, "[{key}] ");
    } else {
        let _ = write!(out, "{key}: ");
    }

    // Values from fetch_children_raw are {"value": "...", "type": "..."}.
    // After truncate_nested they might be replaced with a plain string.
    if let Some(obj) = val.as_object() {
        let v = obj.get("value").and_then(serde_json::Value::as_str).unwrap_or("");
        let t = obj.get("type").and_then(serde_json::Value::as_str).unwrap_or("");
        if t.is_empty() {
            let _ = writeln!(out, "{v}");
        } else {
            let _ = writeln!(out, "{v} ({t})");
        }
    } else if let Some(s) = val.as_str() {
        // Depth-truncated summary string
        let _ = writeln!(out, "{s}");
    } else {
        let _ = writeln!(out, "{val}");
    }
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

    /// Fetch child variables and build a full (non-truncated) JSON value.
    /// Returns `None` for scalar values (`variablesReference` <= 0).
    /// The bool indicates whether the result is array-like (true) or object-like (false).
    async fn fetch_children_raw(
        &self,
        variables_ref: i64,
        indexed_variables: Option<i64>,
        named_variables: Option<i64>,
    ) -> Result<Option<(serde_json::Value, bool)>, AppError> {
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

        let children: Vec<serde_json::Value> = body
            .get("variables")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|c| {
                let name = c.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
                // Filter out debugpy internal metadata children.
                !matches!(name, "special variables" | "function variables")
                    && name != "len()"
                    && !(name.starts_with("__") && name.ends_with("__"))
            })
            .collect();

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

        if is_array {
            let arr: Vec<serde_json::Value> = children
                .iter()
                .map(|c| {
                    let val = sanitize_debuggee_output(
                        c.get("value")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(""),
                    );
                    let typ = c
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    serde_json::json!({
                        "value": truncate_value(&val, max_var_len),
                        "type": typ,
                    })
                })
                .collect();
            Ok(Some((serde_json::Value::Array(arr), true)))
        } else {
            let mut obj = serde_json::Map::new();
            for c in &children {
                let name = sanitize_debuggee_output(
                    c.get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("?"),
                );
                let val = sanitize_debuggee_output(
                    c.get("value")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                );
                let typ = c
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                obj.insert(
                    name,
                    serde_json::json!({
                        "value": truncate_value(&val, max_var_len),
                        "type": typ,
                    }),
                );
            }
            Ok(Some((serde_json::Value::Object(obj), false)))
        }
    }

    /// Fetch the call stack with auto-injected source context.
    /// Returns `(formatted_text, raw_frames)` for reuse by auto-context.
    async fn fetch_stack_with_source(
        &self,
        thread_id: i64,
        max_frames: usize,
    ) -> Result<(String, Vec<serde_json::Value>), AppError> {
        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;
        let context_lines = state.config.source_context_lines;

        let body = {
            let guard = state.require_client().await?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "stackTrace",
                    Some(serde_json::json!({
                        "threadId": thread_id,
                        "levels": max_frames,
                    })),
                    timeout,
                )
                .await?
        };

        let frames = body
            .get("stackFrames")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

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

        Ok((output, frames))
    }

    /// Get the current call stack with auto-injected source context.
    pub async fn handle_get_stack(
        &self,
        params: GetStackParams,
    ) -> Result<CallToolResult, McpError> {
        let thread_id = self
            .resolve_thread_id(params.thread_id)
            .await
            .map_err(McpError::from)?;

        let (output, frames) = self
            .fetch_stack_with_source(thread_id, params.max_frames)
            .await
            .map_err(McpError::from)?;

        if frames.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No stack frames available",
            )]));
        }

        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Fetch scopes and top-level variables for a single frame.
    /// Best-effort: returns empty string on any DAP error.
    #[allow(clippy::too_many_lines)]
    async fn fetch_scope_locals(
        &self,
        frame_id: i64,
        max_scopes: usize,
        max_vars_per_scope: usize,
    ) -> String {
        if max_scopes == 0 {
            return String::new();
        }

        let state = &self.state;
        let timeout = state.config.dap_timeout_secs;
        let max_var_len = state.config.max_variable_length;

        // Fetch scopes for this frame.
        let scopes_body = match async {
            let guard = state.require_client().await?;
            let client = guard.as_ref().unwrap();
            client
                .send_request_with_timeout(
                    "scopes",
                    Some(serde_json::json!({ "frameId": frame_id })),
                    timeout,
                )
                .await
        }
        .await
        {
            Ok(body) => body,
            Err(e) => {
                warn!("auto-context: failed to fetch scopes: {e}");
                return String::new();
            }
        };

        let scopes = scopes_body
            .get("scopes")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut output = String::new();

        for scope in scopes.iter().take(max_scopes) {
            let scope_name = scope
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Scope");
            let variables_ref = scope
                .get("variablesReference")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);

            if variables_ref <= 0 {
                continue;
            }

            // Fetch variables for this scope.
            let vars_body = match async {
                let guard = state.require_client().await?;
                let client = guard.as_ref().unwrap();
                client
                    .send_request_with_timeout(
                        "variables",
                        Some(serde_json::json!({ "variablesReference": variables_ref })),
                        timeout,
                    )
                    .await
            }
            .await
            {
                Ok(body) => body,
                Err(e) => {
                    warn!("auto-context: failed to fetch variables for scope {scope_name}: {e}");
                    continue;
                }
            };

            let variables = vars_body
                .get("variables")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();

            // Filter debugpy noise (same as fetch_children_raw).
            let filtered: Vec<&serde_json::Value> = variables
                .iter()
                .filter(|v| {
                    let name = v.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
                    !matches!(name, "special variables" | "function variables")
                        && name != "len()"
                        && !(name.starts_with("__") && name.ends_with("__"))
                })
                .take(max_vars_per_scope)
                .collect();

            if filtered.is_empty() {
                continue;
            }

            let _ = writeln!(output, "\n{scope_name}:");
            for var in &filtered {
                let name = sanitize_debuggee_output(
                    var.get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("?"),
                );
                let value = sanitize_debuggee_output(
                    var.get("value")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                );
                let value = truncate_value(&value, max_var_len);
                let type_name = var
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if type_name.is_empty() {
                    let _ = writeln!(output, "  {name}: {value}");
                } else {
                    let _ = writeln!(output, "  {name}: {value} ({type_name})");
                }
            }
        }

        output
    }

    /// Build rich auto-context for a stopped event: stack trace + source + locals.
    /// Best-effort: returns just the header if stack fetch fails.
    pub(super) async fn build_stopped_auto_context(
        &self,
        thread_id: i64,
        header: &str,
    ) -> String {
        let mut result = header.to_string();

        // Fetch stack trace with source snippets.
        let (stack_text, frames) = match self
            .fetch_stack_with_source(thread_id, default_max_frames())
            .await
        {
            Ok(pair) => pair,
            Err(e) => {
                warn!("auto-context: failed to fetch stack: {e}");
                return result;
            }
        };

        if frames.is_empty() {
            return result;
        }

        result.push_str("\n\n");
        result.push_str(&stack_text);

        // Get the top frame's ID for scope/locals lookup.
        if let Some(frame_id) = frames
            .first()
            .and_then(|f| f.get("id"))
            .and_then(serde_json::Value::as_i64)
        {
            let max_scopes = self.state.config.auto_context_max_scopes;
            let max_vars = self.state.config.auto_context_max_vars_per_scope;
            let locals = self.fetch_scope_locals(frame_id, max_scopes, max_vars).await;
            if !locals.is_empty() {
                result.push_str(&locals);
            }
        }

        result
    }
    /// Summarize a raw compound value, caching for pagination if it exceeds limits.
    /// Returns `(sliced_value, is_array, Option<(token, total)>)`.
    async fn summarize_with_pagination(
        &self,
        raw_value: serde_json::Value,
        is_array: bool,
        expression: &str,
        type_name: &str,
    ) -> (serde_json::Value, Option<(String, usize)>) {
        let state = &self.state;
        let (exceeds_limit, total_count, limit) = if is_array {
            let len = raw_value.as_array().map_or(0, Vec::len);
            (len > state.config.max_array_items, len, state.config.max_array_items)
        } else {
            let len = raw_value.as_object().map_or(0, serde_json::Map::len);
            (len > state.config.max_object_keys, len, state.config.max_object_keys)
        };

        if !exceeds_limit {
            return (raw_value, None);
        }

        let entry = CacheEntry {
            value: raw_value.clone(),
            expression: expression.to_string(),
            type_name: type_name.to_string(),
            total_count,
            is_array,
            created_at: SystemTime::now(),
        };
        let token = state.pagination_cache.lock().await.insert(entry);

        // Slice to first page.
        let sliced = if is_array {
            let arr = raw_value.as_array().unwrap();
            serde_json::Value::Array(arr.iter().take(limit).cloned().collect())
        } else {
            let obj = raw_value.as_object().unwrap();
            let map: serde_json::Map<String, serde_json::Value> =
                obj.iter().take(limit).map(|(k, v)| (k.clone(), v.clone())).collect();
            serde_json::Value::Object(map)
        };

        (sliced, Some((token, total_count)))
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
                .fetch_children_raw(variables_ref, indexed, named)
                .await
            {
                Ok(Some((raw_value, is_array))) => {
                    let (sliced, pag_info) = self
                        .summarize_with_pagination(
                            raw_value,
                            is_array,
                            &params.expression,
                            type_name,
                        )
                        .await;
                    let sliced =
                        truncate_nested(&sliced, state.config.max_nesting_depth);
                    let pagination = pag_info.as_ref().map(|(token, total)| {
                        let end = sliced
                            .as_array()
                            .map(Vec::len)
                            .or_else(|| sliced.as_object().map(serde_json::Map::len))
                            .unwrap_or(0);
                        PaginationMeta { token, total: *total, offset: 0, end }
                    });
                    let compact = format_compact(
                        &sliced,
                        is_array,
                        &params.expression,
                        type_name,
                        pagination.as_ref(),
                    );
                    return Ok(CallToolResult::success(vec![Content::text(compact)]));
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

    /// Fetch a page of a previously truncated debug result using a pagination token.
    pub async fn handle_get_page(
        &self,
        params: GetPageParams,
    ) -> Result<CallToolResult, McpError> {
        let state = &self.state;
        let max_depth = state.config.max_nesting_depth;

        let mut cache = state.pagination_cache.lock().await;
        let entry = cache
            .get(&params.token)
            .ok_or_else(|| McpError::from(AppError::PaginationTokenNotFound(params.token.clone())))?;

        let expression = entry.expression.clone();
        let type_name = entry.type_name.clone();
        let total_count = entry.total_count;
        let is_array = entry.is_array;
        let value = entry.value.clone();
        drop(cache);

        let (sliced, offset, end) = if is_array {
            let arr = value.as_array().map_or(&[][..], Vec::as_slice);
            let limit = params.limit.unwrap_or(state.config.max_array_items);
            let offset = params.offset.unwrap_or(limit); // default: next page after initial
            let end = (offset + limit).min(arr.len());
            let page = serde_json::Value::Array(arr[offset..end].to_vec());
            (page, offset, end)
        } else {
            let obj = value.as_object().cloned().unwrap_or_default();
            let keys: Vec<String> = obj.keys().cloned().collect();
            let limit = params.limit.unwrap_or(state.config.max_object_keys);
            let offset = params.offset.unwrap_or(limit);
            let end = (offset + limit).min(keys.len());
            let mut page_obj = serde_json::Map::new();
            for k in &keys[offset..end] {
                if let Some(v) = obj.get(k) {
                    page_obj.insert(k.clone(), v.clone());
                }
            }
            (serde_json::Value::Object(page_obj), offset, end)
        };

        let sliced = truncate_nested(&sliced, max_depth);
        let has_more = end < total_count;
        let pagination = if has_more {
            Some(PaginationMeta {
                token: &params.token,
                total: total_count,
                offset,
                end,
            })
        } else {
            None
        };
        let compact = format_compact(&sliced, is_array, &expression, &type_name, pagination.as_ref());
        Ok(CallToolResult::success(vec![Content::text(compact)]))
    }
}
