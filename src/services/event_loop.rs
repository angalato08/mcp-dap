use futures::StreamExt;
use tokio::io::BufReader;
use tokio::sync::broadcast;
use tokio_util::codec::FramedRead;
use tracing::{debug, error, warn};

use crate::dap::client::PendingMap;
use crate::dap::codec::DapCodec;
use crate::dap::types::DapEvent;

/// Read DAP messages from adapter stdout.
/// - Responses (matching a pending `seq`) are dispatched via oneshot channels.
/// - Events are broadcast to all subscribers (tool handlers awaiting stopped/terminated).
pub async fn run_event_loop(
    reader: FramedRead<BufReader<tokio::process::ChildStdout>, DapCodec>,
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

        let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match msg_type {
            "response" => {
                let seq = msg
                    .get("request_seq")
                    .and_then(|s| s.as_i64())
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
                    .and_then(|e| e.as_str())
                    .unwrap_or("");
                let body = msg.get("body").cloned().unwrap_or(serde_json::Value::Null);

                let event = match event_name {
                    "stopped" => Some(DapEvent::Stopped {
                        thread_id: body.get("threadId").and_then(|t| t.as_i64()).unwrap_or(0),
                        reason: body
                            .get("reason")
                            .and_then(|r| r.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                    }),
                    "continued" => Some(DapEvent::Continued {
                        thread_id: body.get("threadId").and_then(|t| t.as_i64()).unwrap_or(0),
                    }),
                    "exited" => Some(DapEvent::Exited {
                        exit_code: body.get("exitCode").and_then(|c| c.as_i64()).unwrap_or(-1),
                    }),
                    "initialized" => Some(DapEvent::Initialized),
                    "terminated" => Some(DapEvent::Terminated),
                    "output" => Some(DapEvent::Output {
                        category: body
                            .get("category")
                            .and_then(|c| c.as_str())
                            .map(String::from),
                        output: body
                            .get("output")
                            .and_then(|o| o.as_str())
                            .unwrap_or("")
                            .to_string(),
                    }),
                    _ => {
                        debug!("unhandled DAP event: {event_name}");
                        None
                    }
                };

                if let Some(event) = event {
                    if event_tx.send(event).is_err() {
                        debug!("no event subscribers");
                    }
                }
            }
            _ => {
                debug!("unknown DAP message type: {msg_type}");
            }
        }
    }

    debug!("DAP event loop ended");
}
