# TODO

## Phase 1: Validation ✓

- [x] Install `debugpy` (`pip install debugpy`) and run `cargo test -- --ignored` to validate the full flow
- [x] Test with `codelldb` adapter (Rust/C++ debugging)
- [x] Test with `delve` adapter (Go debugging) — added TCP transport support
- [x] Error recovery: clean up session state when adapter crashes mid-session
- [x] Handle launch failure cleanup (kill child, reset state if spawn/initialize/launch fails partway)

## Phase 2: AI Optimizations

- [ ] Wire `summarize_array()` / `summarize_object()` into `debug_evaluate` for structured variable responses
- [ ] Add pagination support to truncated results (pagination token for large arrays/objects)
- [ ] Auto-context injection in stopped events (broadcast source snippets with breakpoint hits)
- [ ] Thread-aware execution: expose thread list, support multi-thread debugging
- [ ] Enrich `debug_get_stack` with local variables per frame

## Phase 3: Distribution & Zero-Config

- [ ] GitHub Actions: cross-compile static binaries (x86_64/aarch64, Linux/Mac/Windows)
- [ ] Bootstrapper mode: auto-download adapter binaries (codelldb, debugpy, delve) if not found
- [ ] SSE transport support (in addition to stdio)
- [ ] `launch.json` config file support (load adapter settings from standard format)

## Technical Debt

- [ ] Replace raw `serde_json::Value` DAP requests with typed `dap-types` structs
- [ ] Add unit tests for `DapCodec` (Content-Length framing edge cases)
- [ ] Add unit tests for `SessionState` transitions
- [ ] Add unit tests for context truncation/source extraction
- [ ] Add `debug_remove_breakpoint` tool (currently can only add, not remove)
