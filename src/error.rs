use std::io;

use rmcp::ErrorData as McpError;

/// Central error type for the mcp-dap-rs application.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("no active debug session")]
    NoSession,

    #[error("a debug session is already active")]
    SessionActive,

    #[error("DAP request timed out after {0}s")]
    DapTimeout(u64),

    #[error("DAP error: {0}")]
    DapError(String),

    #[error("invalid session state transition: {from} → {to}")]
    InvalidState { from: String, to: String },

    #[error("adapter not allowed: {0} (allowed: {1})")]
    UnauthorizedAdapter(String, String),

    #[error("failed to spawn debug adapter: {0}")]
    SpawnFailed(#[source] io::Error),

    #[error("codec error: {0}")]
    Codec(String),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<AppError> for McpError {
    fn from(err: AppError) -> Self {
        match &err {
            AppError::NoSession
            | AppError::SessionActive
            | AppError::InvalidState { .. }
            | AppError::UnauthorizedAdapter(..) => {
                McpError::invalid_params(err.to_string(), None)
            }
            _ => McpError::internal_error(err.to_string(), None),
        }
    }
}
