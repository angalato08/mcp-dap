use std::path::PathBuf;
use mcp_dap_rs::config::Config;
use mcp_dap_rs::state::AppState;
use mcp_dap_rs::tools::DebugServer;
use mcp_dap_rs::tools::launch::{LaunchParams, AdapterTransport};
use mcp_dap_rs::tools::breakpoint::SetBreakpointParams;
use mcp_dap_rs::tools::execution::{ContinueParams, StepParams, StepGranularity};
use std::time::Duration;

fn fixture_path(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    path.to_string_lossy().to_string()
}

fn bin_path(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target/debug/examples");
    path.push(name);
    path.to_string_lossy().to_string()
}

#[tokio::test]
async fn test_mutex_step_timeout() {
    let mut config = Config::default();
    config.dap_timeout_secs = 2; // Short timeout for testing
    let state = AppState::new(config);
    let server = DebugServer::new(state);

    let program = bin_path("mutex_block");
    let source_file = fixture_path("mutex_block.rs");

    // 1. Launch
    let result = server.handle_launch(LaunchParams {
        adapter_path: "/usr/bin/lldb-dap".into(),
        adapter_args: vec![],
        program: program.clone(),
        program_args: vec![],
        cwd: None,
        stop_on_entry: true,
        transport: AdapterTransport::default(),
        extra_launch_args: None,
    }).await.expect("launch should succeed");
    
    println!("Launch result: {:?}", result);

    // 2. Set breakpoint
    let result = server.handle_set_breakpoint(SetBreakpointParams {
        file: source_file.clone(),
        line: 19, // println!("Main thread about to lock");
        condition: None,
    }).await.expect("set_breakpoint should succeed");
    
    println!("Set breakpoint result: {:?}", result);

    // 3. Continue to hit the breakpoint
    let result = server.handle_continue(ContinueParams {
        thread_id: None,
        single_thread: false,
        timeout: None,
    }).await.expect("continue should succeed");
    
    println!("Continue result text: {:?}", result.content.first().and_then(|c| c.raw.as_text()).map(|t| &t.text).unwrap());

    // 4. Step over the println!
    let step_over_println = server.handle_step(StepParams {
        granularity: StepGranularity::Over,
        thread_id: None,
        single_thread: false,
        timeout: Some(5),
    }).await.expect("step over println should succeed");
    println!("Step over println result: {:?}", step_over_println);

    // 5. Step over the mutex lock. This should block because the lock is held.
    // If the timeout works, this should return an error or wait until timeout.
    let step_over_mutex = server.handle_step(StepParams {
        granularity: StepGranularity::Over,
        thread_id: None,
        single_thread: false,
        timeout: Some(3), // 3 seconds timeout
    }).await;
    
    println!("Step over mutex result: {:?}", step_over_mutex);
    assert!(step_over_mutex.is_err() || step_over_mutex.unwrap().is_error == Some(true), "Step should have timed out");
}
