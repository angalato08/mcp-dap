# TODO

## Phase 1: Validation ✓

- [x] Install `debugpy` (`pip install debugpy`) and run `cargo test -- --ignored` to validate the full flow
- [x] Test with `codelldb` adapter (Rust/C++ debugging)
- [x] Test with `delve` adapter (Go debugging) — added TCP transport support
- [x] Error recovery: clean up session state when adapter crashes mid-session
- [x] Handle launch failure cleanup (kill child, reset state if spawn/initialize/launch fails partway)

## Phase 2: AI Optimizations

- [x] Wire `summarize_array()` / `summarize_object()` into `debug_evaluate` for structured variable responses
- [x] Depth-limited truncation (`truncate_nested`) for deeply nested structures
- [x] Thread-aware execution: expose thread list, support multi-thread debugging
- [x] LLM prompt injection sanitizer (`sanitize_debuggee_output`) for untrusted debuggee output
- [x] Pagination for truncated results — when large arrays/objects are summarized (e.g. showing 10 of 10,000 items), provide a pagination token so the agent can request the next page without re-evaluating
- [x] Compact output for `debug_evaluate` and `debug_get_page` — ~85% token reduction vs pretty-printed JSON, debugpy metadata filtering
- [x] Auto-context in stopped events — when a breakpoint hits, proactively attach source snippets to the `DapEvent::Stopped` broadcast so the LLM sees where it stopped without a follow-up `debug_get_stack` call
- [x] Local variables per stack frame — enrich `debug_get_stack` to include scopes and local variables for each frame, giving the LLM full context in one call instead of requiring follow-up `debug_evaluate` calls

## Phase 3: Distribution & Zero-Config

- [ ] GitHub Actions: cross-compile static binaries (x86_64/aarch64, Linux/Mac/Windows)
- [ ] Bootstrapper mode: auto-download adapter binaries (codelldb, debugpy, delve) if not found
- [ ] SSE transport support (in addition to stdio)
- [ ] `launch.json` config file support (load adapter settings from standard format)

## Technical Debt

- [x] Add unit tests for `DapCodec` (Content-Length framing edge cases)
- [x] Add unit tests for `SessionState` transitions
- [x] Add unit tests for context truncation/source extraction
- [x] Add `debug_remove_breakpoint` tool
- [x] Deadlock-safe `force_cleanup` (extract child handle before kill/wait)
- [x] Session phase guards on all tool handlers
- [x] Lower `MAX_DAP_MESSAGE_SIZE` to 1 MB + `MAX_HEADER_SIZE` (8 KB) guard
- [x] `memchr` optimization for codec header search
- [x] Event subscription race fix (subscribe before spawn)
- [x] DAP `CapabilitiesEvent` handling + capabilities storage
- [x] Structured tracing spans (`#[instrument]`)
- [x] Clippy pedantic compliance (`[lints.clippy]` in Cargo.toml)
- [ ] Replace raw `serde_json::Value` DAP requests with typed `dap-types` structs
