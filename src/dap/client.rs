use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use futures::SinkExt;
use tokio::io::BufReader;
use tokio::process::ChildStdin;
use tokio::sync::{Mutex, oneshot};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::dap::codec::DapCodec;
use crate::dap::transport::AdapterProcess;

/// Pending response senders keyed by request `seq`.
pub type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>;

/// DAP client that multiplexes requests/responses over adapter stdio.
pub struct DapClient {
    writer: Mutex<FramedWrite<ChildStdin, DapCodec>>,
    seq: AtomicI64,
    pub pending: PendingMap,
    pub reader: Mutex<Option<FramedRead<BufReader<tokio::process::ChildStdout>, DapCodec>>>,
    pub child: Mutex<tokio::process::Child>,
}

impl DapClient {
    /// Create a new DAP client from a spawned adapter process.
    pub fn new(process: AdapterProcess) -> Self {
        let writer = FramedWrite::new(process.stdin, DapCodec);
        let reader = FramedRead::new(BufReader::new(process.stdout), DapCodec);

        Self {
            writer: Mutex::new(writer),
            seq: AtomicI64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
            reader: Mutex::new(Some(reader)),
            child: Mutex::new(process.child),
        }
    }

    /// Send a DAP request and return a receiver for the response.
    pub async fn send_request(
        &self,
        command: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<oneshot::Receiver<serde_json::Value>, crate::error::AppError> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);

        let mut msg = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
        });

        if let Some(args) = arguments {
            msg["arguments"] = args;
        }

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(seq, tx);

        self.writer.lock().await.send(msg).await?;

        Ok(rx)
    }
}
