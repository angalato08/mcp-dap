# PRD: `mcp-dap-rs` (High-Performance MCP Debugger Bridge)

## 1. Product Vision
An enterprise-grade, zero-dependency Debug Adapter Protocol (DAP) bridge for the Model Context Protocol (MCP). Written in Rust, it allows AI software engineers (agents) to natively debug multiple programming languages (Rust, C++, Python, Go, Node.js) without the runtime requirements of Node.js-based alternatives.

## 2. Core Principles
1.  **Zero-Dependency Edge:** Distributed as a single, statically compiled binary. No `node_modules`, no `npm install`, no Python environment required for the server itself.
2.  **LLM-First Context Protection:** AI agents have finite context windows. The server acts as a "smart proxy," semantically summarizing massive arrays/objects and automatically appending source code snippets to breakpoint events to save agent turns.
3.  **Language Agnostic, Protocol Native:** It does not reinvent debugging. It translates MCP JSON-RPC to standard DAP JSON-RPC, leveraging battle-tested debuggers (`CodeLLDB`, `debugpy`, `delve`).
4.  **High Concurrency:** Built on `tokio` to handle asynchronous debugger events (like multi-threaded breakpoints) without blocking the agent's interaction loop.

---

## 3. System Architecture

```text
[ AI Agent (Claude/Cursor/Gemini) ]
               │
               ▼ (MCP Protocol via Stdio / SSE)
               │
┌──────────────────────────────┐
│        mcp-dap-rs            │
│                              │
│  ┌────────────────────────┐  │
│  │   MCP Tool Handlers    │  │
│  └───────────┬────────────┘  │
│              │               │
│  ┌────────────────────────┐  │
│  │ Context Optimizer (LLM)│◄─┼── (Truncates huge variable outputs, 
│  └───────────┬────────────┘  │    injects local source code context)
│              │               │
│  ┌────────────────────────┐  │
│  │   DAP State Machine    │  │
│  └───────────┬────────────┘  │
└──────────────┼───────────────┘
               │
               ▼ (DAP Protocol via Stdio / TCP)
               │
     ┌─────────┴─────────┐
     │   Debug Adapter   │ (e.g., CodeLLDB, debugpy, delve)
     └─────────┬─────────┘
               │
               ▼
     [ Target User Process ]
```

---

## 4. Exposed MCP Tools (The Agent's API)

To keep the agent focused, the server will expose a simplified, high-level abstraction of the DAP protocol:

*   **`debug_launch`**: Starts a debugging session using a standard `launch.json` configuration or explicit adapter path.
*   **`debug_attach`**: Attaches to an existing process by PID.
*   **`debug_set_breakpoint`**: Sets a breakpoint at a specific file and line. Supports conditional expressions.
*   **`debug_continue`**: Resumes execution until the next breakpoint or process exit.
*   **`debug_step`**: Steps `in`, `out`, or `over` the current line.
*   **`debug_get_stack`**: Retrieves the current call stack, but **crucially**, fetches and includes the surrounding 5 lines of source code for the top frames.
*   **`debug_evaluate`**: Evaluates an expression or reads a variable in the current frame. Includes the **LLM Context Guard** to prevent huge data dumps.

---

## 5. AI-Specific Optimizations

Standard debuggers are designed for human eyeballs. AI agents need data formatted differently. This bridge will provide:

### A. The LLM Context Guard (Semantic Truncation)
When an agent evaluates a complex nested struct or a 10,000-element array, a raw DAP response will blow out the LLM's token limit. 
*   **Feature:** `mcp-dap-rs` will intercept large `variables` responses and summarize them (e.g., `[Array of 10,000 items - Showing first 5...]`). It will provide a pagination token if the agent specifically requests more.

### B. Auto-Context Injection
When a debugger hits a breakpoint, standard DAP just returns `{ line: 42, file: "main.rs" }`. The agent then has to waste a turn calling a `read_file` tool to see the code.
*   **Feature:** The bridge automatically reads `main.rs`, extracts lines 37-47, and sends it *with* the breakpoint notification. This eliminates extra turns.

---

## 6. Recommended Tech Stack
*   **Language:** Rust 1.75+
*   **Async Runtime:** `tokio`
*   **MCP Implementation:** `mcp-sdk-rs` or manual JSON-RPC parsing for tighter control.
*   **DAP Implementation:** `dap-rs` crate (Provides structured types for requests, responses, and events).
*   **Serialization:** `serde` and `serde_json`.

---

## 7. Execution Roadmap

### Phase 1: The Rust MVP (Target: `debugpy` & `CodeLLDB`)
*   Implement Stdio MCP transport.
*   Implement Subprocess DAP spawning (start an adapter executable).
*   Map basic tools: `launch`, `continue`, `step_over`, `evaluate`.
*   *Validation:* Connect Claude Desktop or Gemini CLI to the server and have it successfully step through a simple Rust or Python loop.

### Phase 2: AI Optimizations
*   Implement the **Auto-Context Injection** (reading files on breakpoint stop).
*   Implement the **LLM Context Guard** to safely format and truncate large variables.
*   Support threaded execution handling (DAP thread ID mapping).

### Phase 3: Distribution & "Zero-Config"
*   Setup GitHub Actions to release statically linked binaries (`x86_64`, `aarch64` for Linux/Mac/Windows).
*   Create a "Bootstrapper" mode: If the user requests a Rust debug session but `CodeLLDB` isn't installed, `mcp-dap-rs` automatically downloads the correct adapter binary to a local data folder.
