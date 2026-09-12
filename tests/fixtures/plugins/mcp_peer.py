"""Synthetic MCP peer executed from retained package bytes inside the real sandbox."""
import json
import os
import sys
import time

mode, dialect = sys.argv[1:3]

def emit(value):
    sys.stdout.write(json.dumps(value, separators=(",", ":")) + "\n")
    sys.stdout.flush()

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if "id" not in request:
        continue
    result = {}
    if method == "initialize":
        if mode == "descendant":
            if os.fork() == 0:
                os.setsid()
                if os.fork() != 0:
                    os._exit(0)
                while True:
                    time.sleep(1)
        result = {"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "fixture", "version": "1"}}
        if mode == "version":
            result["protocolVersion"] = "2026-07-28"
    elif method == "tools/list":
        result = {"tools": [json.loads(os.environ["MCP_METADATA"]) if "MCP_METADATA" in os.environ else {"name": "gate", "inputSchema": {"type": "object", "additionalProperties": True}}]}
    elif method == "tools/call":
        data = request["params"]["arguments"].get("input", {})
        if isinstance(data, str):
            data = json.loads(data)
        if mode == "confinement":
            assert not os.path.exists(".env"), "private file exposed"
            assert os.environ.get("MCP_AMBIENT_SECRET") is None, "ambient secret exposed"
            assert not os.path.exists("/etc/ssh/ssh_host_rsa_key"), "host private file exposed"
            import socket
            sock = socket.socket()
            try:
                sock.connect(("127.0.0.1", int(sys.argv[3])))
                raise AssertionError("network authority escaped")
            except OSError:
                pass
            finally:
                sock.close()
        if mode == "stderr":
            sys.stderr.write("synthetic-stderr-secret")
            sys.stderr.flush()
        if mode == "stderr_flood":
            sys.stderr.write("x" * 100000)
            sys.stderr.flush()
        if mode == "capability":
            emit({"jsonrpc": "2.0", "id": "server-request", "method": "sampling/createMessage", "params": {"messages": []}})
            continue
        verdict = {"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny" if mode == "deny" else "allow", "permissionDecisionReason": "controlled"}}
        if dialect == "codex" and mode != "deny":
            verdict["hookSpecificOutput"]["updatedInput"] = data
        result = {"content": [], "structuredContent": verdict}
        if mode == "output_missing":
            del result["structuredContent"]["hookSpecificOutput"]["permissionDecisionReason"]
        if mode == "output_valid":
            result["structuredContent"]["hookSpecificOutput"]["permissionDecisionReason"] = "verified"
        if mode == "output_invalid":
            result["structuredContent"]["hookSpecificOutput"]["permissionDecisionReason"] = "wrong"
        if mode == "text" or mode == "output_text":
            result = {"content": [{"type": "text", "text": json.dumps(verdict)}]}
        if mode == "error":
            result["isError"] = True
        if mode == "oversized":
            result["extra"] = "x" * 100000
    else:
        emit({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "unknown"}})
        continue
    if method == "tools/call" and mode.startswith("coalesced_"):
        response = json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}) + "\n"
        extra = response
        if mode == "coalesced_large":
            extra = json.dumps({"jsonrpc": "2.0", "method": "notifications/message", "params": {"data": "x" * 5000}}) + "\n" + response
        if mode == "coalesced_notification":
            extra = json.dumps({"jsonrpc": "2.0", "method": "notifications/message", "params": {}}) + "\n"
        if mode == "coalesced_stale":
            extra = json.dumps({"jsonrpc": "2.0", "id": 999, "result": result}) + "\n"
        if mode == "coalesced_malformed":
            extra = "not-json\n"
        if mode == "coalesced_partial":
            extra = "{"
        os.write(sys.stdout.fileno(), (response + extra).encode())
        continue
    emit({"jsonrpc": "2.0", "id": request["id"], "result": result})

    if method == "tools/call" and mode == "exit_idle":
        os._exit(0)
    if method == "tools/call" and mode == "duplicate":
        emit({"jsonrpc": "2.0", "id": request["id"], "result": result})
