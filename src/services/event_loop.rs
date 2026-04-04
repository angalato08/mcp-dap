use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio::sync::{Mutex, broadcast};
use tracing::{debug, error, instrument, warn};

use crate::context::sanitize::sanitize_debuggee_output;
use crate::dap::client::{DapReader, DapWriter, PendingMap};
use crate::dap::state_machine::{SessionPhase, SessionState};
use crate::dap::types::DapEvent;

/// Read DAP messages from adapter.
/// - Responses (matching a pending `seq`) are dispatched via oneshot channels.
/// - Events are broadcast to all subscribers (tool handlers awaiting stopped/terminated).
/// - Reverse requests from the adapter are rejected with an error response.
#[instrument(skip_all)]
#[allow(clippy::too_many_lines)]
pub async fn run_event_loop(
    reader: DapReader,
    pending: PendingMap,
    event_tx: broadcast::Sender<DapEvent>,
    writer: Arc<Mutex<DapWriter>>,
    session: Arc<Mutex<SessionState>>,
) {
    let mut reader = reader;

    while let Some(result) = reader.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                error!("DAP read error: {e}");
                break;
            }
        };

        let msg_type = msg
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        match msg_type {
            "response" => {
                let seq = msg
                    .get("request_seq")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(-1);

                if let Some(tx) = pending.lock().await.remove(&seq) {
                    if tx.send(msg).is_err() {
                        warn!("response receiver dropped for seq {seq}");
                    }
                } else {
                    warn!("no pending request for response seq {seq}");
                }
            }
            "event" => {
                let event_name = msg
                    .get("event")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let body = msg.get("body").cloned().unwrap_or(serde_json::Value::Null);

                let event = match event_name {
                    "stopped" => Some(DapEvent::Stopped {
                        thread_id: body
                            .get("threadId")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0),
                        reason: body
                            .get("reason")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown")
                            .to_string(),
                        all_threads_stopped: body
                            .get("allThreadsStopped")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                    }),
                    "continued" => Some(DapEvent::Continued {
                        thread_id: body
                            .get("threadId")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0),
                    }),
                    "exited" => Some(DapEvent::Exited {
                        exit_code: body
                            .get("exitCode")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(-1),
                    }),
                    "initialized" => Some(DapEvent::Initialized),
                    "terminated" => Some(DapEvent::Terminated),
                    "output" => Some(DapEvent::Output {
                        category: body
                            .get("category")
                            .and_then(serde_json::Value::as_str)
                            .map(String::from),
                        output: sanitize_debuggee_output(
                            body.get("output")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or(""),
                        ),
                    }),
                    "capabilities" => Some(DapEvent::Capabilities(body)),
                    _ => {
                        debug!("unhandled DAP event: {event_name} body={:?}", body);
                        None
                    }
                };

                if let Some(event) = event {
                    // Centralized state transitions based on adapter events.
                    let phase = match &event {
                        DapEvent::Stopped { .. } => Some(SessionPhase::Stopped),
                        DapEvent::Continued { .. } => Some(SessionPhase::Running),
                        DapEvent::Exited { .. } | DapEvent::Terminated => {
                            Some(SessionPhase::Terminated)
                        }
                        DapEvent::Initialized => Some(SessionPhase::Running),
                        _ => None,
                    };

                    if let Some(p) = phase {
                        let _ = session.lock().await.transition(p);
                    }

                    if event_tx.send(event).is_err() {
                        debug!("no event subscribers");
                    }
                }
            }
            "request" => {
                // DAP reverse request from the adapter (e.g. runInTerminal).
                // We don't support any — send an error response so the adapter
                // doesn't block waiting forever.
                let req_seq = msg
                    .get("seq")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                let command = msg
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                warn!("unsupported DAP reverse request: {command} (seq={req_seq})");

                let err_resp = serde_json::json!({
                    "seq": 0,
                    "type": "response",
                    "request_seq": req_seq,
                    "command": command,
                    "success": false,
                    "message": format!("mcp-dap-rs does not support reverse request '{command}'"),
                });
                if let Err(e) = writer.lock().await.send(err_resp).await {
                    error!("failed to send reverse-request error response: {e}");
                    break;
                }
            }
            _ => {
                debug!("unknown DAP message type: {msg_type}");
            }
        }
    }

    // Notify subscribers that the adapter connection is gone.
    let _ = event_tx.send(DapEvent::AdapterCrashed);
    debug!("DAP event loop ended");
}
