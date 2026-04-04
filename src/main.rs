use std::fs::OpenOptions;

use anyhow::Result;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use mcp_dap::config::Config;
use mcp_dap::state::AppState;
use mcp_dap::tools::DebugServer;

#[tokio::main]
async fn main() -> Result<()> {
    let log_path = std::env::temp_dir().join("mcp-dap.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|e| panic!("failed to open {}: {e}", log_path.display()));

    // Set a panic hook to log panics to the log file.
    let panic_log_path = log_path.clone();
    std::panic::set_hook(Box::new(move |panic_info| {
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        let location = panic_info.location().map_or_else(
            || "unknown location".to_string(),
            |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
        );

        // We open the file again in the panic hook to ensure we can write to it even if the main handle is locked or closed.
        if let Ok(mut file) = OpenOptions::new().append(true).open(&panic_log_path) {
            use std::io::Write;
            let _ = writeln!(file, "PANIC occurred at {location}: {message}");
            let _ = file.flush();
        }
    }));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false);

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with(file_layer)
        .init();

    tracing::info!("mcp-dap-rs starting");

    let config = Config::default();
    let state = AppState::new(config);
    let cleanup_state = state.clone();
    let server = DebugServer::new(state);

    let transport = rmcp::transport::io::stdio();
    let server_handle = server
        .serve(transport)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start server: {e:?}"))?;

    tokio::select! {
        result = server_handle.waiting() => {
            if let Err(e) = result {
                tracing::error!("MCP server crashed with error: {e:?}");
                return Err(anyhow::anyhow!("server crashed: {e:?}"));
            }
            tracing::info!("MCP server shut down cleanly");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("received SIGINT, shutting down");
        }
    }

    // Ensure any active debug adapter subprocess is killed on exit.
    cleanup_state.force_cleanup().await;

    Ok(())
}
