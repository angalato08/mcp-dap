//! Integration tests for DAP adapters.
//!
//! Each test requires its adapter to be installed.
//! Run with: `cargo test -- --ignored`

use std::path::PathBuf;

const CODELLDB_PATH: &str = "/tmp/codelldb/extension/adapter/codelldb";
const DLV_PATH: &str = "/home/angalato/go/bin/dlv";

use mcp_dap_rs::config::Config;
use mcp_dap_rs::state::AppState;
use mcp_dap_rs::tools::DebugServer;

fn fixture_path(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    path.to_string_lossy().to_string()
}

fn make_server() -> DebugServer {
    let config = Config::default();
    let state = AppState::new(config);
    DebugServer::new(state)
}

/// Full end-to-end: launch → set breakpoint → continue → evaluate → step → disconnect.
#[tokio::test]
#[ignore = "requires debugpy: pip install debugpy"]
async fn test_debugpy_step_through_loop() {
    let server = make_server();
    let loop_py = fixture_path("loop.py");

    // 1. Launch
    let result = server
        .handle_launch(mcp_dap_rs::tools::launch::LaunchParams {
            adapter_path: "python3".into(),
            adapter_args: vec!["-m".into(), "debugpy.adapter".into()],
            program: loop_py.clone(),
            program_args: vec![],
            cwd: None,
            stop_on_entry: true,
            transport: Default::default(),
            extra_launch_args: None,
        })
        .await;

    let result = result.expect("launch should succeed");
    let text = content_text(&result);
    assert!(text.contains("stopped at entry"), "got: {text}");

    // 2. Set breakpoint at line 6 (total += i)
    let result = server
        .handle_set_breakpoint(mcp_dap_rs::tools::breakpoint::SetBreakpointParams {
            file: loop_py.clone(),
            line: 6,
            condition: None,
        })
        .await
        .expect("set_breakpoint should succeed");
    let text = content_text(&result);
    assert!(text.contains("Breakpoint"), "got: {text}");

    // 3. Continue to breakpoint
    let result = server
        .handle_continue(mcp_dap_rs::tools::execution::ContinueParams {
            thread_id: None,
            single_thread: false,
        })
        .await
        .expect("continue should succeed");
    let text = content_text(&result);
    assert!(text.contains("Stopped"), "got: {text}");

    // 4. Evaluate `i`
    let result = server
        .handle_evaluate(mcp_dap_rs::tools::inspect::EvaluateParams {
            expression: "i".into(),
            frame_id: None,
        })
        .await
        .expect("evaluate should succeed");
    let text = content_text(&result);
    assert!(text.contains("i ="), "got: {text}");

    // 5. Get stack
    let result = server
        .handle_get_stack(mcp_dap_rs::tools::inspect::GetStackParams {
            thread_id: None,
            max_frames: 10,
        })
        .await
        .expect("get_stack should succeed");
    let text = content_text(&result);
    assert!(text.contains("main"), "got: {text}");

    // 6. Step over
    let result = server
        .handle_step(mcp_dap_rs::tools::execution::StepParams {
            granularity: mcp_dap_rs::tools::execution::StepGranularity::Over,
            thread_id: None,
            single_thread: false,
        })
        .await
        .expect("step should succeed");
    let text = content_text(&result);
    assert!(
        text.contains("Stepped") || text.contains("Stopped"),
        "got: {text}"
    );

    // 7. Disconnect
    let result = server
        .handle_disconnect()
        .await
        .expect("disconnect should succeed");
    let text = content_text(&result);
    assert!(text.contains("disconnected"), "got: {text}");
}

/// CodeLLDB: launch the project binary, stop at entry, inspect stack, disconnect.
#[tokio::test]
#[ignore = "requires codelldb adapter"]
async fn test_codelldb_launch_and_inspect() {
    let server = make_server();

    // Use the project's own debug binary as the debuggee.
    let mut bin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    bin_path.push("target/debug/mcp-dap-rs");
    let program = bin_path.to_string_lossy().to_string();

    // 1. Launch with stop_on_entry
    let result = server
        .handle_launch(mcp_dap_rs::tools::launch::LaunchParams {
            adapter_path: CODELLDB_PATH.into(),
            adapter_args: vec![],
            program: program.clone(),
            program_args: vec![],
            cwd: None,
            stop_on_entry: true,
            transport: Default::default(),
            extra_launch_args: None,
        })
        .await
        .expect("launch should succeed");
    let text = content_text(&result);
    assert!(text.contains("stopped at entry"), "got: {text}");

    // 2. Get stack at entry point
    let result = server
        .handle_get_stack(mcp_dap_rs::tools::inspect::GetStackParams {
            thread_id: None,
            max_frames: 10,
        })
        .await
        .expect("get_stack should succeed");
    let text = content_text(&result);
    assert!(!text.is_empty(), "stack should not be empty, got: {text}");

    // 3. Set breakpoint in main.rs and continue to it
    let mut main_rs = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    main_rs.push("src/main.rs");
    let result = server
        .handle_set_breakpoint(mcp_dap_rs::tools::breakpoint::SetBreakpointParams {
            file: main_rs.to_string_lossy().to_string(),
            line: 18, // `let config = Config::default();`
            condition: None,
        })
        .await
        .expect("set_breakpoint should succeed");
    let text = content_text(&result);
    assert!(text.contains("Breakpoint"), "got: {text}");

    // 4. Continue to breakpoint in main
    let result = server
        .handle_continue(mcp_dap_rs::tools::execution::ContinueParams {
            thread_id: None,
            single_thread: false,
        })
        .await
        .expect("continue should succeed");
    let text = content_text(&result);
    assert!(text.contains("Stopped"), "got: {text}");

    // 5. Stack should now show main
    let result = server
        .handle_get_stack(mcp_dap_rs::tools::inspect::GetStackParams {
            thread_id: None,
            max_frames: 10,
        })
        .await
        .expect("get_stack should succeed");
    let text = content_text(&result);
    assert!(
        text.contains("main"),
        "stack should contain main, got: {text}"
    );

    // 6. Evaluate an expression
    let result = server
        .handle_evaluate(mcp_dap_rs::tools::inspect::EvaluateParams {
            expression: "1 + 1".into(),
            frame_id: None,
        })
        .await
        .expect("evaluate should succeed");
    let text = content_text(&result);
    assert!(
        !text.is_empty(),
        "evaluate should return something, got: {text}"
    );

    // 7. Step over
    let result = server
        .handle_step(mcp_dap_rs::tools::execution::StepParams {
            granularity: mcp_dap_rs::tools::execution::StepGranularity::Over,
            thread_id: None,
            single_thread: false,
        })
        .await
        .expect("step should succeed");
    let text = content_text(&result);
    assert!(
        text.contains("Stepped") || text.contains("Stopped"),
        "got: {text}"
    );

    // 8. Disconnect
    let result = server
        .handle_disconnect()
        .await
        .expect("disconnect should succeed");
    let text = content_text(&result);
    assert!(text.contains("disconnected"), "got: {text}");
}

/// Delve: launch a Go binary via TCP, stop at entry, set breakpoint, continue, evaluate, disconnect.
#[tokio::test]
#[ignore = "requires delve: go install github.com/go-delve/delve/cmd/dlv@latest"]
async fn test_delve_step_through_loop() {
    use mcp_dap_rs::tools::launch::AdapterTransport;

    let server = make_server();
    let loop_go = fixture_path("loop.go");
    let loop_go_bin = fixture_path("loop_go");

    // 1. Launch with TCP transport (delve DAP mode)
    let result = server
        .handle_launch(mcp_dap_rs::tools::launch::LaunchParams {
            adapter_path: DLV_PATH.into(),
            adapter_args: vec!["dap".into()],
            program: loop_go_bin.clone(),
            program_args: vec![],
            cwd: None,
            stop_on_entry: true,
            transport: AdapterTransport::Tcp(44712),
            extra_launch_args: Some(serde_json::json!({"mode": "exec"})),
        })
        .await
        .expect("launch should succeed");
    let text = content_text(&result);
    assert!(text.contains("stopped at entry"), "got: {text}");

    // 2. Set breakpoint at line 9 (total += i)
    let result = server
        .handle_set_breakpoint(mcp_dap_rs::tools::breakpoint::SetBreakpointParams {
            file: loop_go.clone(),
            line: 9,
            condition: None,
        })
        .await
        .expect("set_breakpoint should succeed");
    let text = content_text(&result);
    assert!(text.contains("Breakpoint"), "got: {text}");

    // 3. Continue to breakpoint
    let result = server
        .handle_continue(mcp_dap_rs::tools::execution::ContinueParams {
            thread_id: None,
            single_thread: false,
        })
        .await
        .expect("continue should succeed");
    let text = content_text(&result);
    assert!(text.contains("Stopped"), "got: {text}");

    // 4. Evaluate `i`
    let result = server
        .handle_evaluate(mcp_dap_rs::tools::inspect::EvaluateParams {
            expression: "i".into(),
            frame_id: None,
        })
        .await
        .expect("evaluate should succeed");
    let text = content_text(&result);
    assert!(text.contains("i ="), "got: {text}");

    // 5. Get stack
    let result = server
        .handle_get_stack(mcp_dap_rs::tools::inspect::GetStackParams {
            thread_id: None,
            max_frames: 10,
        })
        .await
        .expect("get_stack should succeed");
    let text = content_text(&result);
    assert!(text.contains("main"), "got: {text}");

    // 6. Step over
    let result = server
        .handle_step(mcp_dap_rs::tools::execution::StepParams {
            granularity: mcp_dap_rs::tools::execution::StepGranularity::Over,
            thread_id: None,
            single_thread: false,
        })
        .await
        .expect("step should succeed");
    let text = content_text(&result);
    assert!(
        text.contains("Stepped") || text.contains("Stopped"),
        "got: {text}"
    );

    // 7. Disconnect
    let result = server
        .handle_disconnect()
        .await
        .expect("disconnect should succeed");
    let text = content_text(&result);
    assert!(text.contains("disconnected"), "got: {text}");
}

/// Error recovery: launch with a bad adapter path should fail and reset state cleanly.
#[tokio::test]
#[ignore = "error recovery test"]
async fn test_launch_bad_adapter_cleans_up() {
    let server = make_server();

    let result = server
        .handle_launch(mcp_dap_rs::tools::launch::LaunchParams {
            adapter_path: "/nonexistent/codelldb".into(),
            adapter_args: vec![],
            program: "/tmp/doesnotexist".into(),
            program_args: vec![],
            cwd: None,
            stop_on_entry: false,
            transport: Default::default(),
            extra_launch_args: None,
        })
        .await;

    assert!(result.is_err(), "launch with bad adapter should fail");

    // State should be reset — a new launch should not get "session active" error.
    let result = server
        .handle_launch(mcp_dap_rs::tools::launch::LaunchParams {
            adapter_path: "/nonexistent/dlv".into(),
            adapter_args: vec![],
            program: "/tmp/doesnotexist".into(),
            program_args: vec![],
            cwd: None,
            stop_on_entry: false,
            transport: Default::default(),
            extra_launch_args: None,
        })
        .await;

    // Should fail with spawn error, NOT "session already active".
    let err = result.unwrap_err();
    assert!(
        !err.message.contains("active"),
        "state should be clean after failed launch, got: {}",
        err.message
    );
}

/// Error recovery: adapter crash mid-session should clean up state.
#[tokio::test]
#[ignore = "requires codelldb adapter"]
async fn test_adapter_crash_recovery() {
    use mcp_dap_rs::dap::state_machine::SessionPhase;

    let server = make_server();

    let mut bin_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    bin_path.push("target/debug/mcp-dap-rs");
    let program = bin_path.to_string_lossy().to_string();

    // Launch successfully
    let result = server
        .handle_launch(mcp_dap_rs::tools::launch::LaunchParams {
            adapter_path: CODELLDB_PATH.into(),
            adapter_args: vec![],
            program,
            program_args: vec![],
            cwd: None,
            stop_on_entry: true,
            transport: Default::default(),
            extra_launch_args: None,
        })
        .await
        .expect("launch should succeed");
    let text = content_text(&result);
    assert!(text.contains("stopped at entry"), "got: {text}");

    // Force-kill the adapter process to simulate a crash
    {
        let guard = server.state.dap_client.lock().await;
        if let Some(client) = guard.as_ref() {
            if let Some(child) = client.child.lock().await.as_mut() {
                child.kill().await.expect("kill adapter");
            }
        }
    }

    // Give the event loop time to detect the crash and clean up
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // State should be cleaned up — session phase should be Uninitialized
    let phase = server.state.session.lock().await.phase();
    assert_eq!(
        phase,
        SessionPhase::Uninitialized,
        "session should be reset after crash, got: {phase}"
    );

    // Client should be cleared
    assert!(
        server.state.dap_client.lock().await.is_none(),
        "client should be cleared after crash"
    );

    // A new launch should work (no "session active" error)
    // Just verify require_no_client passes
    server
        .state
        .require_no_client()
        .await
        .expect("should be able to start a new session after crash");
}

/// Extract text from the first content item in a CallToolResult.
fn content_text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .first()
        .and_then(|c| c.raw.as_text())
        .map(|t| t.text.to_string())
        .unwrap_or_default()
}
