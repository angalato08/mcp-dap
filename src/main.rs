use std::fs::OpenOptions;

use anyhow::Result;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use mcp_dap_rs::config::Config;
use mcp_dap_rs::state::AppState;
use mcp_dap_rs::tools::DebugServer;

#[tokio::main]
async fn main() -> Result<()> {
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/mcp-dap.log")
        .expect("failed to open /tmp/mcp-dap.log");

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with(stderr_layer)
        .with(file_layer)
        .init();

    tracing::info!("mcp-dap-rs starting");

    let config = Config::default();
    let state = AppState::new(config);
    let cleanup_state = state.clone();
    let server = DebugServer::new(state);

    let transport = rmcp::transport::io::stdio();
    let server_handle = server.serve(transport).await?;

    tokio::select! {
        result = server_handle.waiting() => {
            result?;
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
