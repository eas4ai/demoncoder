#!/usr/bin/env python3
"""Qualify pinned Claude MCP substitution with local peers, not a live provider.

Run: python3 tests/plugin_mcp_source_inputs.py --claude /path/to/claude-2.1.267
The captured source fixture is an independent expected result. This check does
not exercise DemonCoder's MCP runner or satisfy its production evidence gate.
"""

import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
import time


MAX_REQUEST = 2 * 1024 * 1024
FIXTURE = Path(__file__).parent / "fixtures/plugins/claude-mcp-input-source.json"
PEER = r'''
import json, sys
from pathlib import Path
log = Path(sys.argv[1])
for line in sys.stdin:
    if len(line) > 262144:
        raise RuntimeError("oversized MCP request")
    message = json.loads(line)
    with log.open("a") as output:
        output.write(json.dumps(message) + "\n")
    if "id" not in message:
        continue
    method = message.get("method")
    if method == "initialize":
        result = {"protocolVersion": message["params"]["protocolVersion"],
                  "capabilities": {"tools": {}},
                  "serverInfo": {"name": "local-input-probe", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "capture", "description": "Capture synthetic input",
                  "inputSchema": {"type": "object", "additionalProperties": True}}]}
    elif method == "tools/call":
        result = {"content": [{"type": "text", "text": "{}"}], "isError": False}
    elif method == "ping":
        result = {}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": message["id"],
                          "error": {"code": -32601, "message": "unsupported"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
'''


def verify_calls(messages, fixture):
    calls = [message for message in messages if message.get("method") == "tools/call"]
    if len(calls) != 2:
        raise AssertionError(f"expected one hook call and one original call; got {len(calls)}")
    actual = calls[0]["params"]["arguments"]
    if actual != fixture["observed_arguments"]:
        differing = sorted(set(actual) | set(fixture["observed_arguments"]))
        differing = [key for key in differing if actual.get(key) != fixture["observed_arguments"].get(key)]
        raise AssertionError(f"source placeholder result changed: {differing}")
    if calls[1]["params"]["arguments"] != fixture["event"]["tool_input"]:
        raise AssertionError("original typed MCP operation changed")
    return len(actual)


def run(binary, root):
    fixture = json.loads(FIXTURE.read_text())
    with binary.open("rb") as executable:
        binary_digest = hashlib.file_digest(executable, "sha256").hexdigest()
    if binary_digest != fixture["executable_sha256"]:
        raise AssertionError("Claude executable differs from the qualified source pin")
    for directory in ("home", "work"):
        (root / directory).mkdir()
    (root / "peer.py").write_text(PEER)
    (root / "probe.py").write_bytes(Path(__file__).read_bytes())
    (root / "source-fixture.json").write_bytes(FIXTURE.read_bytes())
    requests = []
    peer_errors = []

    class Model(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= MAX_REQUEST or len(requests) >= 4:
                    raise AssertionError("model request count or size exceeded bound")
                request = json.loads(self.rfile.read(length))
                requests.append(request)
                replied = any(
                    isinstance(message.get("content"), list)
                    and any(isinstance(item, dict) and item.get("type") == "tool_result"
                            for item in message["content"])
                    for message in request.get("messages", [])
                )
                events = [{"type": "message_start", "message": {
                    "id": "msg_probe", "type": "message", "role": "assistant",
                    "model": "claude-sonnet-4-6", "content": [], "stop_reason": None,
                    "usage": {"input_tokens": 100, "output_tokens": 0}}}]
                if not replied:
                    deadline = time.monotonic() + 3
                    while time.monotonic() < deadline:
                        log = root / "mcp.jsonl"
                        if log.exists() and "tools/list" in log.read_text():
                            break
                        time.sleep(.02)
                    arguments = dict(fixture["event"]["tool_input"])
                    # Send the pre-JavaScript values, independently of the rounded capture.
                    arguments["one"] = 1.0
                    arguments["large"] = 9007199254740993
                    block = {"type": "tool_use", "id": "tool_probe",
                             "name": "mcp__probe__capture", "input": {}}
                    delta = {"type": "input_json_delta", "partial_json": json.dumps(arguments)}
                    stop = "tool_use"
                else:
                    block = {"type": "text", "text": ""}
                    delta = {"type": "text_delta", "text": "done"}
                    stop = "end_turn"
                events.extend([
                    {"type": "content_block_start", "index": 0, "content_block": block},
                    {"type": "content_block_delta", "index": 0, "delta": delta},
                    {"type": "content_block_stop", "index": 0},
                    {"type": "message_delta", "delta": {"stop_reason": stop,
                     "stop_sequence": None}, "usage": {"output_tokens": 30}},
                    {"type": "message_stop"},
                ])
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                for event in events:
                    self.wfile.write(("event: " + event["type"] + "\ndata: "
                                      + json.dumps(event) + "\n\n").encode())
                self.wfile.flush()
            except Exception as error:
                peer_errors.append(str(error))
                self.close_connection = True

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    (root / "settings.json").write_text(json.dumps({"hooks": {"PreToolUse": [{
        "matcher": "mcp__probe__capture", "hooks": [{"type": "mcp_tool", "server": "probe",
        "tool": "capture", "input": fixture["template"], "timeout": 5}]}]}}))
    (root / "mcp-config.json").write_text(json.dumps({"mcpServers": {"probe": {
        "type": "stdio", "command": "/usr/bin/python3",
        "args": [str(root / "peer.py"), str(root / "mcp.jsonl")]}}}))
    environment = {
        "PATH": "/usr/bin:/bin", "HOME": str(root / "home"),
        "CLAUDE_CONFIG_DIR": str(root / "home"), "ANTHROPIC_API_KEY": "synthetic-key",
        "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{server.server_port}",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "DISABLE_TELEMETRY": "1",
        "DISABLE_ERROR_REPORTING": "1", "DISABLE_AUTOUPDATER": "1",
    }
    command = [str(binary), "-p", "Perform the requested fixture operation once.",
               "--output-format", "stream-json", "--verbose", "--model", "claude-sonnet-4-6",
               "--tools", "Write", "--allowedTools", "Write,mcp__probe__capture",
               "--strict-mcp-config", "--mcp-config", str(root / "mcp-config.json"),
               "--setting-sources", "", "--settings", str(root / "settings.json")]
    process = None
    try:
        with (root / "stdout").open("wb") as output, (root / "stderr").open("wb") as errors:
            process = subprocess.Popen(command, cwd=root / "work", env=environment,
                                       stdout=output, stderr=errors, start_new_session=True)
            code = process.wait(timeout=30)
        if code != 0:
            raise AssertionError(f"pinned Claude exited {code}; inspect retained stderr")
        if peer_errors or len(requests) != 2:
            raise AssertionError(f"local model exchange failed: {peer_errors}, requests={len(requests)}")
        messages = [json.loads(line) for line in (root / "mcp.jsonl").read_text().splitlines()]
        fields = verify_calls(messages, fixture)
        # Demonstrate that the verifier rejects a changed source result.
        changed = json.loads(json.dumps(messages))
        call = next(message for message in changed if message.get("method") == "tools/call")
        call["params"]["arguments"]["large"] = "9007199254740993"
        try:
            verify_calls(changed, fixture)
        except AssertionError:
            pass
        else:
            raise AssertionError("source comparison accepted the deliberate numeric violation")
        return {"kind": "controlled-pinned-source", "fields": fields,
                "model_requests": len(requests), "mcp_calls": 2,
                "numeric_violation_rejected": True, "executable_sha256": binary_digest}
    finally:
        if process is not None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        (root / "model-requests.json").write_text(json.dumps(requests, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claude", type=Path, required=True)
    args = parser.parse_args()
    root = Path(tempfile.mkdtemp(prefix="demoncoder-mcp-source-check-"))
    print(f"Evidence: {root}", flush=True)
    result = run(args.claude.resolve(strict=True), root)
    result["artifacts"] = [{"path": item.name,
                            "sha256": hashlib.sha256(item.read_bytes()).hexdigest()}
                           for item in sorted(root.iterdir()) if item.is_file()]
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
