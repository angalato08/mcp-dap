/// DAP events re-broadcast to tool handlers via `tokio::sync::broadcast`.
#[derive(Debug, Clone)]
pub enum DapEvent {
    /// Adapter has finished initializing and is ready for configuration.
    Initialized,
    Stopped {
        thread_id: i64,
        reason: String,
        all_threads_stopped: bool,
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
    /// Adapter capabilities changed mid-session.
    Capabilities(serde_json::Value),
    /// Adapter process exited unexpectedly (event loop stream ended).
    AdapterCrashed,
}
