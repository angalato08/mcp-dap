use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::{Mutex, oneshot};
use tokio_util::codec::{FramedRead, FramedWrite};

use tracing::instrument;

use crate::dap::codec::DapCodec;
use crate::dap::transport::AdapterProcess;
use crate::error::AppError;

/// Boxed reader/writer types for transport-agnostic DAP communication.
pub type DapReader = FramedRead<BufReader<Box<dyn AsyncRead + Send + Unpin>>, DapCodec>;
pub type DapWriter = FramedWrite<Box<dyn AsyncWrite + Send + Unpin>, DapCodec>;

/// Pending response senders keyed by request `seq`.
pub type PendingMap = Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>;

/// DAP client that multiplexes requests/responses over a transport (stdio or TCP).
pub struct DapClient {
    writer: Arc<Mutex<DapWriter>>,
    seq: AtomicI64,
    pub pending: PendingMap,
    pub reader: Mutex<Option<DapReader>>,
    pub child: Mutex<Option<tokio::process::Child>>,
}

impl DapClient {
    /// Create a new DAP client from a spawned adapter process (stdio transport).
    pub fn new(process: AdapterProcess) -> Self {
        let writer: Box<dyn AsyncWrite + Send + Unpin> = Box::new(process.stdin);
        let reader: Box<dyn AsyncRead + Send + Unpin> = Box::new(process.stdout);

        Self {
            writer: Arc::new(Mutex::new(FramedWrite::new(writer, DapCodec))),
            seq: AtomicI64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
            reader: Mutex::new(Some(FramedRead::new(BufReader::new(reader), DapCodec))),
            child: Mutex::new(Some(process.child)),
        }
    }

    /// Create a new DAP client from a TCP stream (for adapters like delve).
    pub fn from_stream(
        read_half: impl AsyncRead + Send + Unpin + 'static,
        write_half: impl AsyncWrite + Send + Unpin + 'static,
        child: tokio::process::Child,
    ) -> Self {
        let writer: Box<dyn AsyncWrite + Send + Unpin> = Box::new(write_half);
        let reader: Box<dyn AsyncRead + Send + Unpin> = Box::new(read_half);

        Self {
            writer: Arc::new(Mutex::new(FramedWrite::new(writer, DapCodec))),
            seq: AtomicI64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
            reader: Mutex::new(Some(FramedRead::new(BufReader::new(reader), DapCodec))),
            child: Mutex::new(Some(child)),
        }
    }

    /// Get a cloneable handle to the DAP writer (for the event loop to send reverse-request error responses).
    pub fn writer_handle(&self) -> Arc<Mutex<DapWriter>> {
        self.writer.clone()
    }

    /// Send a DAP request and return a receiver for the response.
    #[instrument(skip(self, arguments), fields(command = %command))]
    pub async fn send_request(
        &self,
        command: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<oneshot::Receiver<serde_json::Value>, AppError> {
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

    /// Send the standard DAP `initialize` request with mcp-dap-rs client info.
    #[instrument(skip(self))]
    pub async fn send_initialize(&self, timeout_secs: u64) -> Result<serde_json::Value, AppError> {
        self.send_request_with_timeout(
            "initialize",
            Some(serde_json::json!({
                "clientID": "mcp-dap-rs",
                "clientName": "mcp-dap-rs",
                "adapterID": "mcp-dap-rs",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "pathFormat": "path",
            })),
            timeout_secs,
        )
        .await
    }

    /// Send a DAP request, wait for the response with a timeout, and validate success.
    /// Returns the response `body` on success.
    pub async fn send_request_with_timeout(
        &self,
        command: &str,
        arguments: Option<serde_json::Value>,
        timeout_secs: u64,
    ) -> Result<serde_json::Value, AppError> {
        let rx = self.send_request(command, arguments).await?;

        let response = tokio::time::timeout(Duration::from_secs(timeout_secs), rx)
            .await
            .map_err(|_| AppError::DapTimeout(timeout_secs))?
            .map_err(|_| AppError::DapError("response channel closed".into()))?;

        let success = response
            .get("success")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        if !success {
            let message = response
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown DAP error");
            return Err(AppError::DapError(message.to_string()));
        }

        Ok(response
            .get("body")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }
}
