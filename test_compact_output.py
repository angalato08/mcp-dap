#!/usr/bin/env python3
"""
End-to-end test: launches mcp-dap-rs, starts a debugpy session,
evaluates variables, and prints the compact output to verify formatting.
"""

import json
import os
import subprocess
import sys

MCP_DAP_BIN = "./target/release/mcp-dap-rs"
TEST_SCRIPT = "./test_compact.py"
BREAKPOINT_LINE = 14


def send(proc, msg_dict):
    """Send a JSON-RPC message (newline-delimited)."""
    line = json.dumps(msg_dict) + "\n"
    proc.stdin.write(line.encode())
    proc.stdin.flush()


def read_response(proc):
    """Read a newline-delimited JSON-RPC response."""
    while True:
        line = proc.stdout.readline()
        if not line:
            raise EOFError("MCP server closed stdout")
        line = line.strip()
        if not line:
            continue
        msg = json.loads(line)
        # Skip notifications (no "id" field) — wait for actual response
        if "id" in msg:
            return msg
        # Print notifications for visibility
        if msg.get("method"):
            pass  # silently skip server notifications


def call_tool(proc, tool_name, args, req_id):
    send(proc, {
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {"name": tool_name, "arguments": args},
        "id": req_id,
    })
    return read_response(proc)


def main():
    test_script = os.path.abspath(TEST_SCRIPT)

    print(f"Starting MCP server: {MCP_DAP_BIN}")
    proc = subprocess.Popen(
        [MCP_DAP_BIN],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )

    try:
        # Initialize
        send(proc, {
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {"name": "test", "version": "0.1"},
                "capabilities": {},
            },
            "id": 0,
        })
        read_response(proc)
        send(proc, {"jsonrpc": "2.0", "method": "notifications/initialized"})
        print("MCP initialized\n")

        # Launch debugpy
        print("=== debug_launch ===")
        resp = call_tool(proc, "debug_launch", {
            "adapter_path": "python3",
            "adapter_args": ["-m", "debugpy.adapter"],
            "program": test_script,
            "cwd": os.path.dirname(test_script),
            "stop_on_entry": True,
        }, 1)
        print(resp["result"]["content"][0]["text"])
        print()

        # Set breakpoint
        print("=== debug_set_breakpoint ===")
        resp = call_tool(proc, "debug_set_breakpoint", {
            "file": test_script,
            "line": BREAKPOINT_LINE,
        }, 2)
        print(resp["result"]["content"][0]["text"])
        print()

        # Continue to breakpoint
        print("=== debug_continue ===")
        resp = call_tool(proc, "debug_continue", {}, 3)
        print(resp["result"]["content"][0]["text"])
        print()

        # --- Test compact output ---

        # 1. big_list (50 items — should paginate)
        print("=" * 50)
        print("=== debug_evaluate: x (big_list, 50 items) ===")
        print("=" * 50)
        resp = call_tool(proc, "debug_evaluate", {"expression": "x"}, 4)
        text = resp["result"]["content"][0]["text"]
        print(text)
        print(f"  -> {len(text)} bytes, {text.count(chr(10))} lines\n")

        # Extract pagination token
        token = None
        for line in text.splitlines():
            if "[page:" in line:
                token = line.split("[page:")[1].strip().rstrip("]").strip()
                break

        if token:
            # 2. Get next page
            print("=" * 50)
            print(f"=== debug_get_page: token={token} ===")
            print("=" * 50)
            resp = call_tool(proc, "debug_get_page", {"token": token}, 5)
            text = resp["result"]["content"][0]["text"]
            print(text)
            print(f"  -> {len(text)} bytes, {text.count(chr(10))} lines\n")

        # 3. big_dict (30 keys — should paginate)
        print("=" * 50)
        print("=== debug_evaluate: y (big_dict, 30 keys) ===")
        print("=" * 50)
        resp = call_tool(proc, "debug_evaluate", {"expression": "y"}, 6)
        text = resp["result"]["content"][0]["text"]
        print(text)
        print(f"  -> {len(text)} bytes, {text.count(chr(10))} lines\n")

        # 4. small_list (3 items — no pagination)
        print("=" * 50)
        print("=== debug_evaluate: z (small_list, 3 items) ===")
        print("=" * 50)
        resp = call_tool(proc, "debug_evaluate", {"expression": "z"}, 7)
        text = resp["result"]["content"][0]["text"]
        print(text)
        print(f"  -> {len(text)} bytes, {text.count(chr(10))} lines\n")

        # 5. nested dict
        print("=" * 50)
        print("=== debug_evaluate: n (nested) ===")
        print("=" * 50)
        resp = call_tool(proc, "debug_evaluate", {"expression": "n"}, 8)
        text = resp["result"]["content"][0]["text"]
        print(text)
        print(f"  -> {len(text)} bytes, {text.count(chr(10))} lines\n")

        # Disconnect
        print("=== debug_disconnect ===")
        resp = call_tool(proc, "debug_disconnect", {}, 9)
        print(resp["result"]["content"][0]["text"])

    finally:
        proc.terminate()
        proc.wait(timeout=5)

    print("\nDone.")


if __name__ == "__main__":
    main()
