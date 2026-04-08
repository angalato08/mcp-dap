# mcp-dap

An MCP server that bridges AI agents to debug adapters via the Debug Adapter Protocol (DAP).

## Overview

`mcp-dap` is a high-performance, single-binary MCP (Model Context Protocol) server written in Rust. It allows AI coding agents -- such as Claude, Cursor, or Gemini -- to natively launch, control, and inspect debugging sessions across multiple programming languages. Rather than reinventing debugging, it translates MCP tool calls into standard DAP requests, leveraging battle-tested debug adapters.

## Supported Debug Adapters

| Adapter   | Languages          |
|-----------|--------------------|
| CodeLLDB  | Rust, C, C++       |
| debugpy   | Python             |
| Delve     | Go                 |

Additional adapters can be enabled via the `allowed_adapters` configuration option.

## Features

- **Single static binary** -- no Node.js, no Python runtime, no `npm install` required for the server itself.
- **LLM context guard** -- automatically truncates large variable values, arrays, and deeply nested objects to protect the agent's context window. Provides pagination tokens for retrieving additional data on demand.
- **Auto-context injection** -- when the debuggee stops at a breakpoint, the server reads the surrounding source lines and includes them in the response, eliminating extra round-trips.
- **Full session lifecycle** -- launch programs, attach to running processes, set/remove breakpoints, step through code, evaluate expressions, inspect threads and call stacks, and cleanly disconnect.
- **Async and concurrent** -- built on Tokio, handles asynchronous debugger events without blocking the agent interaction loop.
- **Adapter allow-list** -- restricts which debug adapter executables may be spawned, providing a security boundary.

## Installation

### Quick install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/angalato08/mcp-dap/main/install.sh | sh
```

To install a specific version or to a custom directory:

```bash
VERSION=0.2.0 INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/angalato08/mcp-dap/main/install.sh | sh
```

### Pre-built binaries

Download a binary for your platform from the [GitHub Releases](https://github.com/angalato08/mcp-dap/releases) page.

### From source

```bash
git clone https://github.com/angalato08/mcp-dap.git
cd mcp-dap
cargo install --path .
```

## Configuration

`mcp-dap` accepts configuration as a JSON object. All fields have sensible defaults and can be omitted.

```json
{
  "max_variable_length": 1000,
  "source_context_lines": 5,
  "dap_timeout_secs": 30,
  "max_array_items": 10,
  "max_object_keys": 10,
  "max_nesting_depth": 3,
  "pagination_cache_max_entries": 50,
  "pagination_cache_ttl_secs": 300,
  "auto_context_max_scopes": 3,
  "auto_context_max_vars_per_scope": 20,
  "allowed_adapters": ["codelldb", "debugpy", "dlv", "python", "python3", "node", "lldb-dap"],
  "github_repo": "angalato08/mcp-dap",
  "github_allowed_labels": ["bug", "enhancement", "question"]
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `max_variable_length` | `1000` | Maximum character length for variable values before truncation |
| `source_context_lines` | `5` | Number of source lines shown above and below a breakpoint hit |
| `dap_timeout_secs` | `30` | Timeout in seconds for DAP requests |
| `max_array_items` | `10` | Maximum array elements shown before truncation with pagination |
| `max_object_keys` | `10` | Maximum object keys shown before truncation with pagination |
| `max_nesting_depth` | `3` | Maximum nesting depth for variable expansion |
| `pagination_cache_max_entries` | `50` | Maximum number of cached pagination entries |
| `pagination_cache_ttl_secs` | `300` | Time-to-live in seconds for pagination cache entries |
| `auto_context_max_scopes` | `3` | Maximum scopes to expand for auto-context locals (0 disables) |
| `auto_context_max_vars_per_scope` | `20` | Maximum top-level variables per scope in auto-context output |
| `allowed_adapters` | See above | Allowed debug adapter basenames. Empty list disables the allow-list |
| `github_repo` | `"angalato08/mcp-dap"` | GitHub repository in `owner/repo` format for `debug_create_issue`. Empty string disables the tool |
| `github_allowed_labels` | `["bug", "enhancement", "question"]` | Allowed issue labels. Empty list allows any label |

## Usage

`mcp-dap` communicates over stdio using the MCP protocol. Configure it as an MCP server in your AI agent or IDE.

### Claude Desktop / Claude Code

```json
{
  "mcpServers": {
    "mcp-dap": {
      "command": "mcp-dap",
      "args": []
    }
  }
}
```

### Cursor

```json
{
  "mcpServers": {
    "mcp-dap": {
      "command": "mcp-dap",
      "args": []
    }
  }
}
```

## Available MCP Tools

| Tool | Description |
|------|-------------|
| `debug_launch` | Start a debugging session by launching a program under a debug adapter |
| `debug_attach` | Attach to an already-running process by PID |
| `debug_set_breakpoint` | Set a breakpoint at a file and line, with an optional condition |
| `debug_remove_breakpoint` | Remove a breakpoint at a file and line |
| `debug_continue` | Resume execution until the next breakpoint or process exit |
| `debug_step` | Step in, out, or over the current line |
| `debug_get_stack` | Get the current call stack with surrounding source code context |
| `debug_evaluate` | Evaluate an expression or read a variable in the current debug frame |
| `debug_pause` | Pause execution of one or all threads |
| `debug_threads` | List all threads in the debuggee |
| `debug_get_page` | Fetch the next page of a truncated debug result using a pagination token |
| `debug_disconnect` | End the debug session, terminate the debuggee, and clean up |
| `debug_create_issue` | File a GitHub issue (bug report, feature request, or question) against the configured repo |

## Quick Start Workflow

Tools must be called in this order — each step requires the previous one:

```
debug_launch ──► debug_set_breakpoint ──► debug_continue ──► inspect ──► debug_disconnect
                                               ▲                │
                                               │                ▼
                                               ◄── debug_step ◄─┘
```

1. **`debug_launch`** (or `debug_attach`) — start a debug session
2. **`debug_set_breakpoint`** — set breakpoints at file:line locations
3. **`debug_continue`** — run until a breakpoint hits or the program exits
4. **Inspect stopped state:**
   - `debug_get_stack` — view the call stack with source context
   - `debug_evaluate` — evaluate expressions or read variables
   - `debug_threads` — list all threads
5. **`debug_step`** — step in/out/over, then inspect again
6. **`debug_continue`** — resume to next breakpoint (repeat 3–5)
7. **`debug_disconnect`** — end session and clean up

> Only one debug session can be active at a time. Call `debug_disconnect` before starting a new one.

## Example Session

Debugging a Rust program that panics on a division by zero:

```jsonc
// 1. Launch the program under CodeLLDB
debug_launch({
  "adapter_path": "codelldb",
  "program": "./target/debug/myapp",
  "program_args": ["--input", "data.csv"]
})
// → "Session started (CodeLLDB, pid 48291)"

// 2. Set a breakpoint
debug_set_breakpoint({
  "file": "/home/user/myapp/src/main.rs",
  "line": 42
})
// → "Breakpoint set at src/main.rs:42"

// 3. Run to the breakpoint
debug_continue({})
// → Stopped at src/main.rs:42 — shows source context + local variables

// 4. Inspect a variable
debug_evaluate({ "expression": "divisor" })
// → "divisor = 0 (i32)"

// 5. Step over to the next line
debug_step({ "granularity": "over" })
// → Stopped at src/main.rs:43 — shows updated source context

// 6. Evaluate an expression
debug_evaluate({ "expression": "dividend / (divisor + 1)" })
// → "42 (i32)"

// 7. Clean up
debug_disconnect({})
// → "Session disconnected"
```

## Debug Adapter Setup

mcp-dap requires a debug adapter for each language. Install the adapter(s) you need:

### CodeLLDB (Rust, C, C++)

CodeLLDB is distributed as a VS Code extension. To extract the standalone adapter binary:

```bash
# Download the latest release for your platform
# From: https://github.com/nickovs/codelldb-standalone/releases
# Or extract from the VS Code extension:
code --install-extension vadimcn.vscode-codelldb
# The adapter binary is at:
#   ~/.vscode/extensions/vadimcn.vscode-codelldb-*/adapter/codelldb
```

Add the adapter to your `PATH`, or use the full path in `adapter_path`.

### debugpy (Python)

```bash
pip install debugpy
```

The adapter is invoked as `python -m debugpy.adapter`. In your `debug_launch` call:

```jsonc
debug_launch({
  "adapter_path": "python",
  "adapter_args": ["-m", "debugpy.adapter"],
  "program": "my_script.py"
})
```

### Delve (Go)

```bash
go install github.com/go-delve/delve/cmd/dlv@latest
```

Delve uses TCP transport — the adapter listens on a port and mcp-dap connects to it:

```jsonc
debug_launch({
  "adapter_path": "dlv",
  "adapter_args": ["dap", "--listen", "127.0.0.1:12345"],
  "transport": {"tcp": 12345},
  "program": "./cmd/myapp"
})
```

## Advanced Usage

### TCP Transport

Some adapters (like Delve) communicate over TCP instead of stdio. Use the `transport` parameter:

```jsonc
debug_launch({
  "adapter_path": "dlv",
  "adapter_args": ["dap", "--listen", "127.0.0.1:12345"],
  "transport": {"tcp": 12345},
  "program": "./main.go"
})
```

The adapter starts, listens on the specified port, and mcp-dap connects to it as a TCP client.

### Attaching to a Running Process

Use `debug_attach` to connect to an already-running process by PID:

```jsonc
debug_attach({
  "adapter_path": "codelldb",
  "pid": 12345
})
```

This is useful for debugging long-running servers or processes that are hard to reproduce from a cold start. The adapter must support the DAP `attach` request (CodeLLDB and debugpy do; Delve uses a different attach flow).

### Extra Launch Arguments

Use `extra_launch_args` to pass adapter-specific configuration that gets merged into the DAP launch request:

```jsonc
debug_launch({
  "adapter_path": "codelldb",
  "program": "./target/debug/myapp",
  "extra_launch_args": {
    "env": {"RUST_LOG": "debug"},
    "sourceMap": {"/build": "/src"},
    "initCommands": ["settings set target.x86-disassembly-flavor intel"]
  }
})
```

The exact keys depend on the debug adapter — consult its documentation for supported options.

## Troubleshooting

| Error | Cause | Fix |
|-------|-------|-----|
| `Adapter "foo" is not in the allowed list` | The adapter basename isn't in `allowed_adapters` | Add it to the `allowed_adapters` config array, or clear the array to disable the allow-list |
| `Timed out after N seconds waiting for DAP response` | Program exited before hitting a breakpoint, or the breakpoint is on a line that doesn't execute | Verify the breakpoint is on an executable line; increase `dap_timeout_secs` if the program needs more time to reach it |
| `No active debug session` | Calling inspect/step/continue tools without an active session | Call `debug_launch` or `debug_attach` first |
| `A debug session is already active` | Calling `debug_launch`/`debug_attach` while a session is running | Call `debug_disconnect` to end the current session first |
| `Failed to spawn adapter process` | The adapter binary isn't found or isn't executable | Verify the adapter is installed and on your `PATH`, or use an absolute path in `adapter_path` |
| `Pagination token not found` / `expired` | Using a stale or invalid pagination token with `debug_get_page` | Re-run the original `debug_evaluate` call to get a fresh token; tokens expire after `pagination_cache_ttl_secs` (default 300s) |

## Architecture

```
[ AI Agent (Claude / Cursor / Gemini) ]
               |
               v  MCP Protocol (stdio)
               |
+------------------------------+
|           mcp-dap             |
|                              |
|  +------------------------+  |
|  |   MCP Tool Handlers    |  |
|  +-----------+------------+  |
|              |               |
|  +------------------------+  |
|  | Context Optimizer (LLM)|  |  <- Truncation, source injection
|  +-----------+------------+  |
|              |               |
|  +------------------------+  |
|  |   DAP State Machine    |  |
|  +-----------+------------+  |
+--------------+---------------+
               |
               v  DAP Protocol (stdio)
               |
     +---------+---------+
     |   Debug Adapter    |  (CodeLLDB, debugpy, delve)
     +---------+---------+
               |
               v
     [ Target Process ]
```

## License

This project is licensed under the [MIT License](LICENSE).
