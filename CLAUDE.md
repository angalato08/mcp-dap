# mcp-dap-rs

MCP server bridging AI agents to debug adapters (CodeLLDB, debugpy, delve) via the DAP protocol.

## Build & Check

```bash
cargo check    # type-check
cargo build    # debug build
cargo test     # run tests
```

## Architecture

- `src/main.rs` — Entry point: tracing, config, rmcp stdio server with `tokio::select!`
- `src/lib.rs` — Public module declarations
- `src/config.rs` — `Config` struct with `#[serde(default)]`
- `src/state.rs` — `AppState`: Arc-wrapped shared state + broadcast channels
- `src/error.rs` — `AppError` thiserror enum
- `src/tools/` — MCP tool definitions (agent-facing API) via rmcp `#[tool_router]` / `#[tool_handler]`
- `src/dap/` — DAP client (adapter-facing): codec, transport, state machine, client multiplexer
- `src/context/` — AI optimizations: LLM context guard (truncation), auto-context injection (source snippets)
- `src/services/` — Background async tasks (DAP event loop)

## Conventions

- Tool handlers live in `src/tools/*.rs`, each with typed `*Params` structs deriving `Deserialize + JsonSchema`
- Tool dispatch uses rmcp's `#[tool_router]` macro on `DebugServer` with `#[tool_handler] impl ServerHandler`
- Tool handler return type: `Result<CallToolResult, rmcp::ErrorData>`
- DAP wire protocol uses `tokio_util::codec` with `Content-Length` framing
- Shared state accessed via `AppState` (cloneable, Arc-wrapped internals)
- Errors: use `AppError` for internal errors, `rmcp::ErrorData` at the MCP boundary
- Async runtime: tokio with `features = ["full"]`
