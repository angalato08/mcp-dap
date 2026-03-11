/// DAP events re-broadcast to tool handlers via `tokio::sync::broadcast`.
#[derive(Debug, Clone)]
pub enum DapEvent {
    /// Adapter has finished initializing and is ready for configuration.
    Initialized,
    Stopped {
        thread_id: i64,
        reason: String,
    },
    Continued {
        thread_id: i64,
    },
    Exited {
        exit_code: i64,
    },
    Terminated,
    Output {
        category: Option<String>,
        output: String,
    },
}
