# mcp-dap-rs Project Rules

These rules are foundational mandates for Gemini CLI. They take absolute precedence over general workflows.

## Foundational Mandates — LSP FIRST

**CRITICAL: You MUST use the MCP Language Server tools as your primary means of understanding and navigating code.**

### Rule 1: LSP First — ALWAYS
Before using `read_file`, `grep_search`, or `glob` on source code (`.rs`, `.py`, `.go`, etc.), you MUST:
1. **Discover structure** → `mcp-language-server.document_symbols`
2. **Find symbols** → `mcp-language-server.workspace_symbols`
3. **Navigate definitions** → `mcp-language-server.get_definition`
4. **Understand types** → `mcp-language-server.hover`
5. **Find usages** → `mcp-language-server.get_references`
6. **Trace call graphs** → `mcp-language-server.incoming_calls` / `outgoing_calls`
7. **Validate changes** → `mcp-language-server.diagnostics` immediately after every edit.

### Rule 2: Fallback & Exhaustion Check
You may ONLY use raw file tools (`read_file`, `grep_search`) if the LSP tool was called first and provided insufficient results. 
**Enforcement:** Before reading a source file, you MUST state: 
"LSP Exhaustion Check: I have used LSP tools and they did not provide the required information."

### Rule 3: Refactoring
Use `mcp-language-server.rename_symbol` for all renames to ensure semantic correctness.

## Engineering Standards
- Tool handlers in `src/tools/*.rs`.
- DAP protocol uses `tokio_util::codec`.
- Errors use `AppError` internally, `rmcp::ErrorData` at boundaries.
- Always run `cargo check` and `cargo test` after significant changes.
