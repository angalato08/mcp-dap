# Feedback: MCP DAP (Debug Adapter Protocol)

**Context:** Used during clad Phase 8 Polish and Phase 8b for debugging seccomp handlers, SUD integration, and ELF loader issues. Adapter: lldb-dap.

## What Worked Well — Star Tool

- **Locals dump instantly revealed root causes:**
  - `is_static: false` — showed static binary detection was running after command wrapping (would have been 20+ min of printf debugging)
  - `is_static: true` after fix — confirmed the fix worked in one step
  - Dispatch breakpoint showing which handler was called and with what args

- **Breakpoint hit/miss is diagnostic:** When breakpoints in `handle_recvfrom_dgram` didn't hit, it immediately told us the UDP dispatch path wasn't reached — narrowing the problem.

- **Call stack context:** Seeing the full stack (test → builder → sandbox → handler) with source snippets at each frame made it easy to understand the flow.

## What Didn't Work Well

- **Can't follow forks:** Breakpoints in the forked child (SUD process) never hit. LLDB doesn't follow `fork()` by default, and there's no way to set follow-fork-mode via DAP. Had to use strace for child process debugging.

- **Can't evaluate Rust variables by name:** `notif->data.nr` and `is_static` both failed with "use of undeclared identifier." The Locals dump showed the values, but targeted evaluation didn't work. This is an LLDB-Rust limitation, not a DAP issue.

- **Step-over blocks on mutex:** When stepping over `table.lock().unwrap()` where another thread held the lock, the step timed out after 30s. Had to disconnect and reconnect. No way to set a step timeout or cancel a step.

## Debugging Escalation Pattern (For MCP Workflow PRD)

The most effective debugging approach discovered across Phases 7-8b:

1. **Test output** (behavior level) — "what failed?"
2. **strace** (syscall level) — "what syscalls happened?" Found the rt_sigreturn loop, the ioctl(FIONBIO) gap, the SIGSEGV after rseq.
3. **DAP** (source level) — "what values do the variables have?" Found `is_static: false`, confirmed dispatch paths, verified fix worked.

Each level answers different questions. The MCP workflow should recommend escalating through them when a test fails:
- Test fails → check output first
- Output unclear → strace the failing process
- strace shows unexpected behavior → DAP to inspect source-level state

## Recommendations for MCP Workflow Server

- Suggest DAP when a test fails in **Step 5 (Implementation)** with an unclear error.
- Auto-suggest strace for sandbox/process-related failures.
- The workflow could store "debugging playbooks" — e.g., "for seccomp issues, set breakpoint at socket_handler.rs:dispatch."
