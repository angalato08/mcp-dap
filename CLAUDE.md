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

# Foundational Mandates — MCP Language Server (MANDATORY)

**CRITICAL: These rules are NON-NEGOTIABLE. You MUST follow them for every code-related task. Violating these rules means you are not doing your job correctly.**

### Rule 1: LSP First — ALWAYS

Before using `Read`, `Grep`, `Glob`, or any Agent/Explore subagent on source code, you MUST first use the MCP Language Server tools. This is not a suggestion — it is a hard requirement.

**Required workflow for ANY code task:**
1. **Discover structure** → `document_symbols` before reading any source file (`.go`, `.h`, `.cpp`, `.py`, `.ts`, `.rs`, etc.)
2. **Find symbols** → `workspace_symbols` before using Grep or Glob to search for code
3. **Navigate definitions** → `get_definition` instead of reading files and scanning manually
4. **Understand types** → `hover` to check types, signatures, and documentation
5. **Find usages** → `get_references` / `references` instead of Grep for call sites
6. **Find implementations** → `get_implementation` / `implementation` for virtual/abstract methods
7. **Trace call graphs** → `incoming_calls` / `outgoing_calls` for understanding data flow; use `dependency_graph` to visualize multi-level call chains when understanding how components connect
8. **Validate changes** → `diagnostics` / `workspace_diagnostics` after every code edit

### Rule 2: Fallback Conditions

You may ONLY use `Read`, `Grep`, `Glob`, or raw file tools for source code when:
- The LSP tool was called first AND returned insufficient results, OR
- The task is purely textual (e.g., editing CLAUDE.md, README, .fbs schemas, CMakeLists.txt, Makefile), OR
- The language server is confirmed unavailable (call `server_status` to check)

**ENFORCEMENT MECHANISM:** Before you execute `read_file` or `grep_search` on any source file (`.go`, `.h`, `.cpp`, `.py`, `.ts`, `.rs`, etc.), you MUST write the following exact phrase in your text response:
"LSP Exhaustion Check: I confirm I have already used `api_overview`, `workspace_symbols`, or `get_definition` and it did not provide the required information."
If you cannot truthfully output this sentence, you are forbidden from using `read_file` or `grep_search`.

**If you catch yourself reaching for Read/Grep/Glob on a source file without having called an LSP tool first — STOP and use the LSP tool instead.**

### Rule 3: Subagents Must Also Use LSP

When delegating code exploration to an Agent (e.g., Explore subagent), explicitly instruct it to use MCP Language Server tools (`document_symbols`, `workspace_symbols`, `get_definition`, `hover`, `references`) as its primary means of exploration. Do NOT let subagents default to raw file reads and grep.

### Rule 4: Validation After Edits

After writing or editing any source file (`.go`, `.h`, `.cpp`, `.py`, `.ts`, `.rs`), you MUST call `diagnostics` on the modified file to catch compile errors immediately. Do not wait for the user to build.

### Rule 5: Refactoring & Renaming — USE THE TOOL

When asked to rename a variable, function, or struct across the codebase, you MUST use `rename_symbol` instead of manual `replace` operations. 

- This ensures semantic correctness and prevents "accidental" renames of similarly named symbols in different scopes.
- Only fall back to manual replacement if the LSP server returns an error or does not support renaming for that specific symbol.
- After a rename, always call `workspace_diagnostics` to verify the entire project's integrity.

