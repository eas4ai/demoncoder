#!/usr/bin/env python3
"""Qualify installed Claude safe-mode context and explicit SDK inspection tools."""

import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import tempfile
import threading
import time


def case(binary, safe):
    extra = ["--setting-sources", "user,project,local"] + (
        ["--safe-mode"] if safe else []
    )
    with tempfile.TemporaryDirectory(prefix="hook-claude-isolation-") as tmp:
        root = Path(tmp)
        home = root / "home"
        cwd = root / "empty"
        home.mkdir()
        cwd.mkdir()
        (home / "CLAUDE.md").write_text("GLOBAL_MEMORY_CANARY_73519")
        (cwd / "CLAUDE.md").write_text("WORKSPACE_MEMORY_CANARY_73519")
        requests = []

        class Peer(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_POST(self):
                data = json.loads(
                    self.rfile.read(int(self.headers.get("Content-Length", 0)))
                )
                if not self.path.split("?")[0].endswith("/messages"):
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(b'{"input_tokens":1}')
                    return
                requests.append(data)
                first = len(requests) == 1
                block = (
                    {
                        "type": "tool_use",
                        "id": "tool_1",
                        "name": "mcp__demoncoder__inspect",
                        "input": {},
                    }
                    if first
                    else {"type": "text", "text": ""}
                )
                rows = [
                    {
                        "type": "message_start",
                        "message": {
                            "id": "msg_probe",
                            "type": "message",
                            "role": "assistant",
                            "model": "claude-sonnet-4-6",
                            "content": [],
                            "stop_reason": None,
                            "usage": {"input_tokens": 10, "output_tokens": 0},
                        },
                    },
                    {"type": "content_block_start", "index": 0, "content_block": block},
                ]
                if not first:
                    rows.append(
                        {
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": {"type": "text_delta", "text": '{"ok":true}'},
                        }
                    )
                rows += [
                    {"type": "content_block_stop", "index": 0},
                    {
                        "type": "message_delta",
                        "delta": {
                            "stop_reason": "tool_use" if first else "end_turn",
                            "stop_sequence": None,
                        },
                        "usage": {"output_tokens": 10},
                    },
                    {"type": "message_stop"},
                ]
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                for row in rows:
                    self.wfile.write(
                        (
                            "event: "
                            + row["type"]
                            + "\ndata: "
                            + json.dumps(row)
                            + "\n\n"
                        ).encode()
                    )
                self.wfile.flush()

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Peer)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        env = {
            "PATH": "/usr/bin:/bin",
            "HOME": str(home),
            "CLAUDE_CONFIG_DIR": str(home),
            "CLAUDE_CODE_OAUTH_TOKEN": "synthetic-oauth",
            "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{server.server_port}",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
            "DISABLE_TELEMETRY": "1",
            "DISABLE_ERROR_REPORTING": "1",
            "DISABLE_AUTOUPDATER": "1",
        }
        args = [
            str(binary),
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--tools",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            '{"mcpServers":{"demoncoder":{"type":"sdk","name":"demoncoder"}}}',
            "--setting-sources",
            "",
            "--permission-prompt-tool",
            "stdio",
            "--model",
            "claude-sonnet-4-6",
            *extra,
        ]
        proc = subprocess.Popen(
            args,
            cwd=cwd,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )

        def send(data):
            proc.stdin.write((json.dumps(data) + "\n").encode())
            proc.stdin.flush()

        send(
            {
                "type": "control_request",
                "request_id": "initialize",
                "request": {"subtype": "initialize", "hooks": None, "skills": []},
            }
        )
        select = selectors.DefaultSelector()
        select.register(proc.stdout, selectors.EVENT_READ)
        buffer = b""
        calls = 0
        methods = []
        finished = False
        auth = None
        try:
            deadline = time.monotonic() + 25
            while time.monotonic() < deadline and not finished:
                if not select.select(0.2):
                    continue
                chunk = os.read(proc.stdout.fileno(), 65536)
                if not chunk:
                    break
                buffer += chunk
                while b"\n" in buffer:
                    line, buffer = buffer.split(b"\n", 1)
                    msg = json.loads(line)
                    if (
                        msg.get("type") == "control_response"
                        and msg.get("response", {}).get("request_id") == "initialize"
                    ):
                        send(
                            {
                                "type": "user",
                                "message": {
                                    "role": "user",
                                    "content": "Use inspect, then return a JSON verdict.",
                                },
                                "parent_tool_use_id": None,
                                "session_id": "",
                            }
                        )
                    if msg.get("type") == "system" and msg.get("subtype") == "init":
                        auth = msg.get("apiKeySource")
                    if msg.get("type") == "result":
                        finished = True
                    if msg.get("type") != "control_request":
                        continue
                    req = msg["request"]
                    sub = req.get("subtype")
                    if sub == "can_use_tool":
                        result = {"behavior": "allow", "updatedInput": req["input"]}
                    elif sub == "mcp_message":
                        rpc = req["message"]
                        method = rpc.get("method")
                        methods.append(method)
                        if method == "initialize":
                            result = {
                                "protocolVersion": "2025-03-26",
                                "capabilities": {"tools": {}},
                                "serverInfo": {"name": "demoncoder", "version": "1"},
                            }
                        elif method == "tools/list":
                            result = {
                                "tools": [
                                    {
                                        "name": "inspect",
                                        "description": "Inspect captured evidence",
                                        "inputSchema": {
                                            "type": "object",
                                            "properties": {},
                                        },
                                    }
                                ]
                            }
                        elif method == "tools/call":
                            calls += 1
                            result = {
                                "content": [
                                    {"type": "text", "text": "captured evidence"}
                                ],
                                "isError": False,
                            }
                        else:
                            result = {}
                        result = {
                            "mcp_response": {
                                "jsonrpc": "2.0",
                                "id": rpc.get("id"),
                                "result": result,
                            }
                        }
                    else:
                        result = {}
                    send(
                        {
                            "type": "control_response",
                            "response": {
                                "subtype": "success",
                                "request_id": msg["request_id"],
                                "response": result,
                            },
                        }
                    )
            material = json.dumps(requests)
            summary = {
                "safe_mode": safe,
                "finished": finished,
                "model_requests": len(requests),
                "inspection_calls": calls,
                "global_canary": "GLOBAL_MEMORY_CANARY_73519" in material,
                "workspace_canary": "WORKSPACE_MEMORY_CANARY_73519" in material,
                "auth": auth,
                "methods": methods,
                "tool_names": [x.get("name") for x in requests[0].get("tools", [])]
                if requests
                else [],
            }
            assert finished and len(requests) == 2 and calls == 1, summary
            assert auth == "none", summary
            assert summary["tool_names"] == ["mcp__demoncoder__inspect"], summary
            assert summary["global_canary"] == (not safe), summary
            assert summary["workspace_canary"] == (not safe), summary
            summary["passed"] = True
            return summary
        finally:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
            server.shutdown()
            server.server_close()
            select.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    before = hashlib.sha256(binary.read_bytes()).hexdigest()
    cases = [case(binary, safe) for safe in [False, True]]
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == before
    result = {
        "transport": "actual installed Claude with synthetic local model and OAuth",
        "binary_sha256": before,
        "cases": cases,
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
