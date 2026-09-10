#!/usr/bin/env python3
"""Qualify managed Codex hook context against a synthetic local TLS model."""

import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import tempfile
import threading
import time

sys.dont_write_bytecode = True
# Keep imported fixture sources unchanged during qualification.
from codex_https_fixture import create_server  # noqa: E402
from installed_backends import fake_codex_auth  # noqa: E402


def case(binary, managed, missing_compact_prompt=False):
    with tempfile.TemporaryDirectory(prefix="hook-codex-isolation-") as tmp:
        root = Path(tmp)
        home = root / "codex-home"
        home.mkdir()
        cwd = root / "empty"
        cwd.mkdir()
        fake_codex_auth(home)
        (home / "AGENTS.md").write_text("GLOBAL_MEMORY_CANARY_73519")
        (root / "base.txt").write_text("BASE_MEMORY_CANARY_73519")
        (root / "compact.txt").write_text("COMPACT_MEMORY_CANARY_73519")
        if missing_compact_prompt:
            (root / "compact.txt").unlink()
        (cwd / "AGENTS.md").write_text("WORKSPACE_MEMORY_CANARY_73519")
        config = 'model="gpt-5.4"\ncli_auth_credentials_store="file"\ndeveloper_instructions="DEVELOPER_MEMORY_CANARY_73519"\n'
        notification = root / "notification-ran"
        config += (
            "notify="
            + json.dumps(
                [
                    "/usr/bin/python3",
                    "-c",
                    f"from pathlib import Path; Path({str(notification)!r}).write_text('ran')",
                ]
            )
            + "\n"
        )
        config += (
            "model_instructions_file="
            + json.dumps(str(root / "base.txt"))
            + "\nexperimental_compact_prompt_file="
            + json.dumps(str(root / "compact.txt"))
            + "\n[features]\nenable_request_compression=false\n"
        )
        (home / "config.toml").write_text(config)
        requests = []

        class Peer(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"models":[],"items":[]}')

            def do_POST(self):
                data = json.loads(
                    self.rfile.read(int(self.headers.get("Content-Length", 0)))
                )
                self.send_response(200)
                if not self.path.endswith("/responses"):
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(b"{}")
                    return
                requests.append(data)
                first = len(requests) == 1
                compact = (
                    json.loads(
                        data.get("client_metadata", {}).get(
                            "x-codex-turn-metadata", "{}"
                        )
                    ).get("request_kind")
                    == "compaction"
                )
                item = (
                    {
                        "type": "function_call",
                        "id": "fc_probe",
                        "call_id": "call_probe",
                        "name": "snapshot_read",
                        "arguments": '{"path":"candidate.txt"}',
                    }
                    if first
                    else {
                        "type": "message",
                        "id": "msg_probe",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": '{"ok":true}'}],
                    }
                )
                if compact:
                    item = {
                        "type": "compaction",
                        "encrypted_content": "SYNTHETIC COMPACTED CONTEXT",
                    }
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                rows = [
                    {
                        "type": "response.created",
                        "response": {
                            "id": "resp_probe",
                            "status": "in_progress",
                            "output": [],
                        },
                    },
                    {
                        "type": "response.output_item.added",
                        "output_index": 0,
                        "item": item,
                    },
                    {
                        "type": "response.output_item.done",
                        "output_index": 0,
                        "item": item,
                    },
                    {
                        "type": "response.completed",
                        "response": {
                            "id": "resp_probe",
                            "status": "completed",
                            "output": [item],
                            "usage": {
                                "input_tokens": 10,
                                "output_tokens": 5,
                                "total_tokens": 15,
                            },
                        },
                    },
                ]
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

        server = create_server(root / "tls", Peer)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        env = {
            "PATH": "/usr/bin:/bin",
            "HOME": str(root),
            "CODEX_HOME": str(home),
            "CODEX_CA_CERTIFICATE": str(server.ca_certificate),
            "NO_PROXY": "",
        }
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"]:
            env[key] = f"http://127.0.0.1:{server.server_port}"
        if managed:
            env["CODEX_DEMONCODER_MODEL_HOOK"] = "v1"
        args = [binary, "app-server", "--stdio"]
        for feature in [
            "shell_tool",
            "view_image",
            "apps",
            "plugins",
            "hooks",
            "multi_agent",
            "browser_use",
            "computer_use",
            "image_generation",
            "skill_mcp_dependency_install",
            "skill_search",
            "workspace_dependencies",
            "memories",
            "goals",
            "request_permissions_tool",
        ]:
            args += ["--disable", feature]
        for setting in [
            "mcp_servers={}",
            'web_search="disabled"',
            'forced_login_method="chatgpt"',
            "tools.experimental_request_user_input.enabled=false",
            "tools.update_plan.enabled=false",
            "orchestrator.skills.enabled=false",
            "orchestrator.mcp.enabled=false",
        ]:
            args += ["-c", setting]
        error = open(root / "errors", "wb")
        proc = subprocess.Popen(
            args,
            cwd=cwd,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=error,
            start_new_session=True,
        )

        def send(data):
            proc.stdin.write((json.dumps(data) + "\n").encode())
            proc.stdin.flush()

        send(
            {
                "id": 1,
                "method": "initialize",
                "params": {
                    "clientInfo": {"name": "demoncoder", "version": "0.1"},
                    "capabilities": {"experimentalApi": True},
                },
            }
        )
        select = selectors.DefaultSelector()
        select.register(proc.stdout, selectors.EVENT_READ)
        buffer = b""
        calls = 0
        finished = False
        account = None
        errors = []
        thread = None
        compacting = False
        rejected = False
        try:
            deadline = time.monotonic() + 35
            while time.monotonic() < deadline and not finished and not rejected:
                if not select.select(0.2):
                    continue
                chunk = os.read(proc.stdout.fileno(), 65536)
                if not chunk:
                    break
                buffer += chunk
                while b"\n" in buffer:
                    line, buffer = buffer.split(b"\n", 1)
                    msg = json.loads(line)
                    if "error" in msg:
                        errors.append(msg["error"])
                        rejected = missing_compact_prompt and not managed
                    if msg.get("id") == 1 and "result" in msg:
                        send({"method": "initialized", "params": {}})
                        send(
                            {
                                "id": 2,
                                "method": "account/read",
                                "params": {"refreshToken": False},
                            }
                        )
                    if msg.get("id") == 2 and "result" in msg:
                        account = msg["result"].get("account", {}).get("type")
                        send(
                            {
                                "id": 3,
                                "method": "thread/start",
                                "params": {
                                    "model": "gpt-5.4",
                                    "cwd": str(cwd),
                                    "sandbox": "workspace-write",
                                    "approvalPolicy": "never",
                                    "config": {"mcp_servers": {}},
                                    "environments": [],
                                    "experimentalRawEvents": False,
                                    "dynamicTools": [
                                        {
                                            "type": "function",
                                            "name": "snapshot_read",
                                            "description": "Read captured snapshot",
                                            "inputSchema": {
                                                "type": "object",
                                                "properties": {
                                                    "path": {"type": "string"}
                                                },
                                                "required": ["path"],
                                                "additionalProperties": False,
                                            },
                                        }
                                    ],
                                },
                            }
                        )
                    if msg.get("id") == 3 and "result" in msg:
                        thread = msg["result"]["thread"]["id"]
                        send(
                            {
                                "id": 4,
                                "method": "turn/start",
                                "params": {
                                    "threadId": msg["result"]["thread"]["id"],
                                    "input": [
                                        {
                                            "type": "text",
                                            "text": "Read candidate then return verdict.",
                                        }
                                    ],
                                    "environments": [],
                                },
                            }
                        )
                    if msg.get("method") == "item/tool/call":
                        calls += 1
                        send(
                            {
                                "id": msg["id"],
                                "result": {
                                    "success": True,
                                    "contentItems": [
                                        {
                                            "type": "inputText",
                                            "text": "captured candidate",
                                        }
                                    ],
                                },
                            }
                        )
                    if msg.get("method") == "turn/completed":
                        assert msg["params"]["turn"]["status"] == "completed", msg
                        if compacting:
                            finished = True
                        else:
                            compacting = True
                            send(
                                {
                                    "id": 5,
                                    "method": "thread/compact/start",
                                    "params": {"threadId": thread},
                                }
                            )
            material = json.dumps(requests)
            if missing_compact_prompt and not managed:
                assert not requests and not finished
                assert "experimental compact prompt file" in json.dumps(errors), errors
                return {
                    "managed": False,
                    "missing_compact_prompt": True,
                    "startup_rejected": True,
                    "passed": True,
                }
            summary = {
                "managed": managed,
                "missing_compact_prompt": missing_compact_prompt,
                "finished": finished,
                "model_requests": len(requests),
                "inspection_calls": calls,
                "canaries": {
                    name: name + "_MEMORY_CANARY_73519" in material
                    for name in ["GLOBAL", "BASE", "DEVELOPER", "COMPACT", "WORKSPACE"]
                },
                "account": account,
                "errors": errors,
                "notification_ran": notification.exists(),
                "tools": [
                    x.get("name", x.get("type")) for x in requests[0].get("tools", [])
                ]
                if requests
                else [],
            }
            assert finished and len(requests) == 3 and calls == 1, summary
            assert account == "chatgpt" and not errors, summary
            assert summary["tools"] == ["snapshot_read"], summary
            for name in ["GLOBAL", "BASE", "DEVELOPER"]:
                assert summary["canaries"][name] == (not managed), summary
            assert summary["notification_ran"] == (not managed), summary
            assert not summary["canaries"]["WORKSPACE"], summary
            summary["passed"] = True
            return summary
        finally:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait()
            error.close()
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
    cases = [
        case(str(binary), managed, missing)
        for missing in [False, True]
        for managed in [False, True]
    ]
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == before
    result = {
        "transport": "actual managed Codex with synthetic local TLS model and login",
        "binary_sha256": before,
        "cases": cases,
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
