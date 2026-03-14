---
name: Use MCP tools directly, not Python wrappers
description: When asked to "try it out" or test debug functionality, use the MCP DAP tools directly instead of writing Python test scripts
type: feedback
---

Use the actual MCP DAP tools (debug_launch, debug_evaluate, etc.) directly when testing debug functionality, not Python wrapper scripts.

**Why:** The user wants to see the tools exercised in-context, not through an intermediary.

**How to apply:** When asked to test/try out debug features, call the MCP tools directly.
