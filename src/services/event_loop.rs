use futures::StreamExt;
use tokio::sync::broadcast;
use tracing::{debug, error, instrument, warn};

use crate::context::sanitize::sanitize_debuggee_output;
use crate::dap::client::{DapReader, PendingMap};
use crate::dap::types::DapEvent;

/// Read DAP messages from adapter.
/// - Responses (matching a pending `seq`) are dispatched via oneshot channels.
/// - Events are broadcast to all subscribers (tool handlers awaiting stopped/terminated).
#[instrument(skip_all)]
pub async fn run_event_loop(
    reader: DapReader,
    pending: PendingMap,
    event_tx: broadcast::Sender<DapEvent>,
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

        let msg_type = msg.get("type").and_then(serde_json::Value::as_str).unwrap_or("");

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
                        thread_id: body.get("threadId").and_then(serde_json::Value::as_i64).unwrap_or(0),
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
                        thread_id: body.get("threadId").and_then(serde_json::Value::as_i64).unwrap_or(0),
                    }),
                    "exited" => Some(DapEvent::Exited {
                        exit_code: body.get("exitCode").and_then(serde_json::Value::as_i64).unwrap_or(-1),
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
                        debug!("unhandled DAP event: {event_name}");
                        None
                    }
                };

                if let Some(event) = event
                    && event_tx.send(event).is_err()
                {
                    debug!("no event subscribers");
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
