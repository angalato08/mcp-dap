# Architectural Review & Refactoring Plan

This document outlines areas for improving the modularity, maintainability, and understandability of the `mcp_dap` codebase.

## 1. Separation of Concerns: MCP Layer vs. DAP Protocol

**Current State:**
In files like `src/tools/execution.rs` and `src/tools/launch.rs`, the `DebugServer` handlers map MCP input parameters (like `ContinueParams`) directly to `serde_json::json!` payloads. The MCP handlers act as both API endpoints and protocol serializers, making them intimately aware of the raw DAP JSON protocol structure.

**Recommendation:**
Create a strongly-typed `DapService` or enrich `DapClient` with domain-specific methods.
* Example: Use `client.continue_thread(thread_id, single_thread)` instead of manually creating `{ "threadId": thread_id, "singleThread": true }` inside `handle_continue`.
* **Benefit:** This makes the MCP handlers solely responsible for request unpacking and text formatting (`CallToolResult::success()`), leaving the DAP specifics hidden behind a clean Rust API.

## 2. Duplicated Event Orchestration and Timeouts

**Current State:**
Waiting for specific events (like `Stopped` or `Initialized`) with a timeout is repeated heavily across the codebase. Almost identical boilerplate using `tokio::time::timeout` and `loop { event_rx.recv() }` exists in `handle_continue`, `handle_step`, `handle_pause`, `launch_handshake`, and `attach_handshake`.

**Recommendation:**
Extract a reusable event-waiting utility.
* Example: Create a method like `wait_for_stop_event(timeout)` or `wait_until(matcher, timeout)`.
* **Benefit:** This would remove hundreds of lines of boilerplate and make the flow of execution commands immediately understandable:
  ```rust
  client.send_step(thread_id).await?;
  let event = state.wait_for_stop(timeout).await?;
  self.handle_stopped_event(&event).await
  ```

## 3. Centralized Session State Transitions

**Current State:**
The `SessionPhase` transitions (e.g., `transition(SessionPhase::Running)`) are scattered manually across tool handlers. When an event comes in, the handler catching it transitions the state.

**Recommendation:**
Move the responsibility of state transition into the `event_loop` itself or a dedicated event sink.
* Example: If the `event_loop` receives a `Stopped` event from the debug adapter, it should automatically transition the session state to `Stopped` before broadcasting the event.
* **Benefit:** This prevents state desync bugs if a handler misses an event or crashes, ensuring the core state machine is robustly tied to the actual adapter events.

## 4. `AppState` Grouping (God Object)

**Current State:**
`AppState` holds a mix of global state (`Config`, `PaginationCache`) and session-specific state (`dap_client`, `session`, `breakpoints`, `capabilities`). In `force_cleanup`, these fields are manually reset one by one.

**Recommendation:**
Group the session-specific fields into an `ActiveSession` struct.
```rust
pub struct ActiveSession {
    pub client: DapClient,
    pub phase: SessionPhase,
    pub breakpoints: HashMap<String, Vec<TrackedBreakpoint>>,
    pub capabilities: Option<serde_json::Value>,
}
```
* **Benefit:** `AppState` just holds `Arc<Mutex<Option<ActiveSession>>>`. When a session disconnects or crashes, you just drop the `ActiveSession`, automatically and safely cleaning up all related state without manual `.clear()` calls.
