use mcp_dap::config::Config;
use mcp_dap::state::AppState;
use mcp_dap::tools::DebugServer;
use mcp_dap::tools::breakpoint::SetBreakpointParams;
use mcp_dap::tools::execution::ContinueParams;
use mcp_dap::tools::launch::{AdapterTransport, LaunchParams};
use std::path::PathBuf;

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
async fn test_fork_breakpoint() {
    let config = Config::default();
    let state = AppState::new(config);
    let server = DebugServer::new(state);

    let program = bin_path("fork");
    let source_file = fixture_path("fork.rs");

    // 1. Launch
    let result = server
        .handle_launch(LaunchParams {
            adapter_path: "/usr/bin/lldb-dap".into(),
            adapter_args: vec![],
            program: program.clone(),
            program_args: vec![],
            cwd: None,
            stop_on_entry: true,
            transport: AdapterTransport::default(),
            extra_launch_args: Some(serde_json::json!({
                "initCommands": ["settings set target.process.follow-fork-mode child"]
            })),
        })
        .await
        .expect("launch should succeed");

    println!("Launch result: {:?}", result);

    // 2. Set breakpoint at line 19 (x += 1 in child)
    let result = server
        .handle_set_breakpoint(SetBreakpointParams {
            file: source_file.clone(),
            line: 19,
            condition: None,
        })
        .await
        .expect("set_breakpoint should succeed");

    println!("Set breakpoint result: {:?}", result);

    // 3. Continue
    let result = server
        .handle_continue(ContinueParams {
            thread_id: None,
            single_thread: false,
            timeout: None,
        })
        .await
        .expect("continue should succeed");

    println!(
        "Continue result text: {}",
        result
            .content
            .first()
            .and_then(|c| c.raw.as_text())
            .map(|t| &t.text)
            .unwrap()
    );

    // 4. Evaluate is_static
    let result = server
        .handle_evaluate(mcp_dap::tools::inspect::EvaluateParams {
            expression: "is_static".into(),
            frame_id: None,
            context: None,
        })
        .await
        .expect("evaluate is_static should succeed");
    println!("Evaluate is_static: {:?}", result);

    // 5. Evaluate notif.data.nr
    let result = server
        .handle_evaluate(mcp_dap::tools::inspect::EvaluateParams {
            expression: "notif.data.nr".into(),
            frame_id: None,
            context: None,
        })
        .await
        .expect("evaluate notif.data.nr should succeed");
    println!("Evaluate notif.data.nr: {:?}", result);

    // 6. Test raw command via repl context
    let result = server
        .handle_evaluate(mcp_dap::tools::inspect::EvaluateParams {
            expression: "settings show target.process.follow-fork-mode".into(),
            frame_id: None,
            context: Some("repl".into()),
        })
        .await
        .expect("repl evaluate should succeed");
    println!("Repl evaluate result: {:?}", result);
}
