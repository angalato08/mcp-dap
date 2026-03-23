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

### From source

```bash
git clone https://github.com/angalato08/mcp-dap.git
cd mcp-dap
cargo install --path .
```

### Pre-built binaries

Download a binary for your platform from the [GitHub Releases](https://github.com/angalato08/mcp-dap/releases) page.

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
  "allowed_adapters": ["codelldb", "debugpy", "dlv", "python", "python3", "node", "lldb-dap"]
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

## Usage

`mcp-dap` communicates over stdio using the MCP protocol. Configure it as an MCP server in your AI agent or IDE.

### Claude Desktop / Claude Code

```json
{
  "mcpServers": {
    "mcp-dap": {
      "command": "mcp-dap-rs",
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
      "command": "mcp-dap-rs",
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

## Architecture

```
[ AI Agent (Claude / Cursor / Gemini) ]
               |
               v  MCP Protocol (stdio)
               |
+------------------------------+
|         mcp-dap-rs           |
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
