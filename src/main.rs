use anyhow::Result;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use mcp_dap_rs::config::Config;
use mcp_dap_rs::state::AppState;
use mcp_dap_rs::tools::DebugServer;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("mcp-dap-rs starting");

    let config = Config::default();
    let state = AppState::new(config);
    let server = DebugServer::new(state);

    let transport = rmcp::transport::io::stdio();
    let server_handle = server.serve(transport).await?;

    tokio::select! {
        result = server_handle.waiting() => {
            result?;
            tracing::info!("MCP server shut down cleanly");
        }
    }

    Ok(())
}
