//! Integration test: launch debugpy, set breakpoint, step through a loop.
//!
//! Requires `debugpy` installed: `pip install debugpy`
//! Run with: `cargo test -- --ignored`

use std::path::PathBuf;

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
        .handle_continue(mcp_dap_rs::tools::execution::ContinueParams { thread_id: None })
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

/// Extract text from the first content item in a CallToolResult.
fn content_text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .first()
        .and_then(|c| c.raw.as_text())
        .map(|t| t.text.to_string())
        .unwrap_or_default()
}
