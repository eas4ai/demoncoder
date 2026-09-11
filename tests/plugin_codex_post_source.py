#!/usr/bin/env python3
"""Qualify pinned Codex dynamic-tool post frames with a local model peer.

This source check does not exercise DemonCoder's production lifecycle dispatcher.
Run with --codex /path/to/qualified/managed/codex.
"""

import argparse
import copy
import hashlib
import http.server
import json
import os
import re
from pathlib import Path
import selectors
import shlex
import signal
import subprocess
import tempfile
import threading
import time

from codex_https_fixture import create_server
from installed_backends import fake_codex_auth


FIXTURE = Path(__file__).parent / "fixtures/plugins/codex-post-source.json"
MAX_BYTES = 2 * 1024 * 1024
CORRECTION_MARKER = "[Plugin-origin fixture correction]"
CAPTURE = '''import json, sys
from pathlib import Path
raw = sys.stdin.buffer.read(262145)
if len(raw) > 262144:
    raise RuntimeError("oversized source event")
message = json.loads(raw)
with Path(sys.argv[1]).open("a") as output:
    output.write(json.dumps(message) + "\\n")
'''


def correction_prompt(fixture):
    return (CORRECTION_MARKER + " The host interrupted the backend for this plugin correction, not a developer denial. "
            "Any backend cancellation placeholder describes the interruption, not the actual tool effect. "
            "Host tool completed with original evidence: "
            + fixture["response"] + ". Continue remaining work without repeating the completed tool.")


def verify_correction(trace, requests, fixture, thread, turn):
    interrupts = [i for i, row in enumerate(trace) if row["direction"] == "host"
                  and row["message"].get("method") == "turn/interrupt"]
    acknowledgments = [i for i, row in enumerate(trace) if row["direction"] == "backend"
                       and row["message"].get("id") == 5 and "result" in row["message"]]
    results = [i for i, row in enumerate(trace) if row["direction"] == "backend"
               and row["message"].get("method") == "turn/completed"]
    followups = [i for i, row in enumerate(trace) if row["direction"] == "host"
                and row["message"].get("id") == 6
                and row["message"].get("method") == "turn/start"]
    models = [i for i, row in enumerate(trace) if row["direction"] == "model"]
    assert len(interrupts) == len(acknowledgments) == len(followups) == 1
    assert len(results) == len(models) == len(requests) == 2
    assert models[0] < interrupts[0] < acknowledgments[0] < followups[0] < models[1]
    assert interrupts[0] < results[0] < followups[0] < results[1]
    interrupt = trace[interrupts[0]]["message"]["params"]
    assert interrupt["threadId"] == thread and interrupt["turnId"] == turn
    first = trace[results[0]]["message"]["params"]
    second = trace[results[1]]["message"]["params"]
    assert first["threadId"] == second["threadId"] == thread
    assert first["turn"]["id"] == turn and first["turn"]["status"] == "interrupted"
    assert second["turn"]["id"] != turn and second["turn"]["status"] == "completed"
    followup = trace[followups[0]]["message"]["params"]
    assert followup["threadId"] == thread
    assert followup["input"] == [{"type": "text", "text": correction_prompt(fixture)}]
    # These full-prompt notifications pass through the bounded production reader,
    # even though correction acknowledgment uses the turn/start RPC response.
    notifications = [row["message"] for row in trace[followups[0] + 1:results[1]]
                     if row["direction"] == "backend"
                     and row["message"].get("method") in ["item/started", "item/completed"]
                     and row["message"]["params"]["item"]["type"] == "userMessage"]
    assert [message["method"] for message in notifications] == ["item/started", "item/completed"]
    item_ids = []
    for message, time_field in zip(notifications, ["startedAtMs", "completedAtMs"]):
        assert set(message) == {"method", "params", "emittedAtMs"}
        params = message["params"]
        assert set(params) == {"item", "threadId", "turnId", time_field}
        assert params["threadId"] == thread and params["turnId"] == second["turn"]["id"]
        assert re.fullmatch(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", params["turnId"])
        for timestamp in [message["emittedAtMs"], params[time_field]]:
            assert timestamp is None or type(timestamp) is int and 0 <= timestamp <= 2**63 - 1
        item = params["item"]
        assert set(item) == {"type", "id", "clientId", "content"}
        assert item["clientId"] is None
        assert re.fullmatch(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", item["id"])
        assert item["content"] == [{"type": "text", "text": correction_prompt(fixture), "text_elements": []}]
        item_ids.append(item["id"])
    assert item_ids[0] == item_ids[1]
    assert any(item.get("role") == "user" and any(
        block.get("text") == correction_prompt(fixture) for block in item.get("content", []))
        for item in requests[1]["input"])
    tool_request = next(row["message"]["id"] for row in trace if row["direction"] == "backend"
                        and row["message"].get("method") == "item/tool/call")
    assert not any(row["direction"] == "host" and row["message"].get("id") == tool_request
                   and "result" in row["message"] for row in trace)


def verify(hooks, requests, calls, fixture, success, thread, turn, cwd, transcript,
           correction=False, trace=None):
    expected = ["PreToolUse", "PostToolUse"] if success and not correction else ["PreToolUse"]
    assert [item["hook_event_name"] for item in hooks] == expected
    assert len(calls) == 1 and len(requests) == 2
    call = calls[0]
    assert call["callId"] == fixture["tool_use_id"]
    assert call["threadId"] == thread and call["turnId"] == turn
    assert call["arguments"] == fixture["arguments"] and call["tool"] == fixture["tool_name"]
    for hook in hooks:
        assert hook["session_id"] == thread and hook["turn_id"] == turn
        assert hook["cwd"] == str(cwd)
        assert hook["tool_use_id"] == fixture["tool_use_id"]
        assert hook["tool_name"] == fixture["tool_name"]
        assert hook["tool_input"] == fixture["arguments"]
        assert hook["permission_mode"] == fixture["permission_mode"]
        assert hook["transcript_path"] == transcript
        assert isinstance(hook["transcript_path"], str) and Path(hook["transcript_path"]).is_file()
    if correction:
        verify_correction(trace, requests, fixture, thread, turn)
        return
    if success:
        assert hooks[-1]["tool_response"] == fixture["response"]
    results = [item for item in requests[-1]["input"] if item.get("type") == "function_call_output"]
    assert len(results) == 1 and results[0]["call_id"] == fixture["tool_use_id"]
    assert results[0]["output"] == fixture["response"]


def run_case(binary, root, fixture, success, correction=False):
    root.mkdir()
    home, cwd = root / "codex-home", root / "work"
    home.mkdir()
    cwd.mkdir()
    fake_codex_auth(home)
    script = root / "capture.py"
    script.write_text(CAPTURE)
    hook_path = root / "hooks.jsonl"
    command = shlex.join(["/usr/bin/python3", str(script), str(hook_path)])
    config = 'model="gpt-5.4"\ncli_auth_credentials_store="file"\n[features]\nenable_request_compression=false\nhooks=true\n'
    for event in ["PreToolUse", "PostToolUse"]:
        config += '[[hooks.' + event + ']]\nmatcher="capture"\nhooks=[{type="command",command=' + json.dumps(command) + ',timeout=5}]\n'
    (home / "config.toml").write_text(config)
    requests, errors, events, calls, trace = [], [], [], [], []

    class Peer(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"models":[],"items":[]}')

        def do_POST(self):
            try:
                length = int(self.headers.get("Content-Length", "0"))
                assert 0 < length <= MAX_BYTES
                data = json.loads(self.rfile.read(length))
                self.send_response(200)
                if not self.path.endswith("/responses"):
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(b'{}')
                    return
                assert len(requests) < 2
                trace.append({"direction": "model", "message": data})
                requests.append(data)
                item = ({"type": "function_call", "id": "fc_source", "call_id": fixture["tool_use_id"],
                         "name": fixture["tool_name"], "arguments": json.dumps(fixture["arguments"])}
                        if len(requests) == 1 else {"type": "message", "id": "msg_source", "role": "assistant",
                         "content": [{"type": "output_text", "text": "done"}]})
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                rows = [
                    {"type": "response.created", "response": {"id": "resp_source", "status": "in_progress", "output": []}},
                    {"type": "response.output_item.added", "output_index": 0, "item": item},
                    {"type": "response.output_item.done", "output_index": 0, "item": item},
                    {"type": "response.completed", "response": {"id": "resp_source", "status": "completed", "output": [item],
                     "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}}},
                ]
                for row in rows:
                    self.wfile.write(("event: " + row["type"] + "\ndata: " + json.dumps(row) + "\n\n").encode())
                self.wfile.flush()
            except Exception as error:
                errors.append(repr(error))
                self.close_connection = True

    server = create_server(root / "tls", Peer)
    server.daemon_threads = True
    peer_thread = threading.Thread(target=server.serve_forever, daemon=True)
    peer_thread.start()
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(root), "CODEX_HOME": str(home),
                   "CODEX_CA_CERTIFICATE": str(server.ca_certificate), "NO_PROXY": ""}
    for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"]:
        environment[key] = f"http://127.0.0.1:{server.server_port}"
    args = [str(binary), "app-server", "--stdio"]
    for feature in ["shell_tool", "view_image", "apps", "plugins", "multi_agent", "browser_use",
                    "computer_use", "image_generation", "skill_mcp_dependency_install", "skill_search",
                    "workspace_dependencies", "memories", "goals", "request_permissions_tool"]:
        args += ["--disable", feature]
    for setting in ['mcp_servers={}', 'web_search="disabled"', 'forced_login_method="chatgpt"',
                    'tools.experimental_request_user_input.enabled=false', 'tools.update_plan.enabled=false',
                    'orchestrator.skills.enabled=false', 'orchestrator.mcp.enabled=false']:
        args += ["-c", setting]
    process = None
    selector = selectors.DefaultSelector()
    try:
        with (root / "stderr").open("wb") as stderr:
            process = subprocess.Popen(args, cwd=cwd, env=environment, stdin=subprocess.PIPE,
                                       stdout=subprocess.PIPE, stderr=stderr, start_new_session=True)

            def send(message):
                trace.append({"direction": "host", "message": message})
                process.stdin.write((json.dumps(message) + "\n").encode())
                process.stdin.flush()

            send({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "demoncoder", "version": "0.1"},
                  "capabilities": {"experimentalApi": True}}})
            selector.register(process.stdout, selectors.EVENT_READ)
            buffer, received = b"", 0
            deadline, finished = time.monotonic() + 35, False
            thread, turn, transcript = None, None, None
            first_done = False
            interrupt_ack = False
            correction_sent = False
            interrupt_started = None
            corrected_turn = None
            while time.monotonic() < deadline and not finished:
                if not selector.select(.1):
                    continue
                chunk = os.read(process.stdout.fileno(), 65536)
                assert chunk, "backend closed before completion"
                received += len(chunk)
                assert received <= MAX_BYTES, "backend output exceeded bound"
                buffer += chunk
                while b"\n" in buffer:
                    line, buffer = buffer.split(b"\n", 1)
                    message = json.loads(line)
                    trace.append({"direction": "backend", "message": message})
                    events.append(message)
                    assert "error" not in message, f"source RPC failed: {message.get('error')}"
                    if message.get("id") == 1 and "result" in message:
                        send({"method": "initialized", "params": {}})
                        send({"id": 2, "method": "hooks/list", "params": {"cwds": [str(cwd)]}})
                    if message.get("id") == 2 and "result" in message:
                        # Authorize only this fixture's exact command hashes in its private config.
                        entries = message["result"]["data"][0]["hooks"]
                        assert len(entries) == 2 and all(hook["command"] == command for hook in entries)
                        for hook in entries:
                            config += '[hooks.state.' + json.dumps(hook["key"]) + ']\ntrusted_hash=' + json.dumps(hook["currentHash"]) + '\n'
                        (home / "config.toml").write_text(config)
                        send({"id": 3, "method": "thread/start", "params": {"model": "gpt-5.4", "cwd": str(cwd),
                              "sandbox": "workspace-write", "approvalPolicy": "never", "config": {"mcp_servers": {}},
                              "environments": [], "experimentalRawEvents": False, "dynamicTools": [{"type": "function",
                              "name": fixture["tool_name"], "description": "Capture fixture input", "inputSchema": {
                                  "type": "object", "properties": {"path": {"type": "string"}},
                                  "required": ["path"], "additionalProperties": False}}]}})
                    if message.get("id") == 3 and "result" in message:
                        thread = message["result"]["thread"]["id"]
                        transcript = message["result"]["thread"]["path"]
                        send({"id": 4, "method": "turn/start", "params": {"threadId": thread,
                              "input": [{"type": "text", "text": "Perform the fixture operation once."}], "environments": []}})
                    if message.get("id") == 4 and "result" in message:
                        turn = message["result"]["turn"]["id"]
                    if message.get("method") == "item/tool/call":
                        calls.append(message["params"])
                        if correction:
                            interrupt_started = time.monotonic()
                            send({"id": 5, "method": "turn/interrupt", "params": {
                                  "threadId": thread, "turnId": message["params"]["turnId"]}})
                        else:
                            send({"id": message["id"], "result": {"success": success,
                                  "contentItems": [{"type": "inputText", "text": fixture["response"]}]}})
                    if message.get("id") == 5 and "result" in message:
                        interrupt_ack = True
                    if message.get("id") == 6 and "result" in message:
                        corrected_turn = message["result"]["turn"]["id"]
                    if message.get("method") == "turn/completed":
                        if correction and not correction_sent:
                            assert message["params"]["turn"]["status"] == "interrupted"
                            first_done = True
                        else:
                            assert message["params"]["turn"]["status"] == "completed"
                            if correction:
                                assert message["params"]["turn"]["id"] == corrected_turn
                            finished = True
                    if first_done and interrupt_ack and not correction_sent:
                        assert len(requests) == 1, "backend crossed pending dynamic result"
                        assert time.monotonic() - interrupt_started < 3, "interrupt did not settle pending call"
                        send({"id": 6, "method": "turn/start", "params": {"threadId": thread,
                              "input": [{"type": "text", "text": correction_prompt(fixture)}], "environments": []}})
                        correction_sent = True
            assert finished and not errors, f"incomplete local exchange: {errors}"
            hooks = [json.loads(line) for line in hook_path.read_text().splitlines()]
            verify(hooks, requests, calls, fixture, success, thread, turn, cwd, transcript, correction, trace)
            changed = copy.deepcopy(hooks)
            changed[0]["tool_use_id"] = "wrong-source-id"
            try:
                verify(changed, requests, calls, fixture, success, thread, turn, cwd, transcript, correction, trace)
            except AssertionError:
                pass
            else:
                raise AssertionError("verifier accepted corrupted source identity")
            changed = copy.deepcopy(hooks)
            if success and not correction:
                changed[-1]["tool_response"] = {"output": fixture["response"]}
            else:
                changed.append({**changed[0], "hook_event_name": "PostToolUse"})
            try:
                verify(changed, requests, calls, fixture, success, thread, turn, cwd, transcript, correction, trace)
            except AssertionError:
                pass
            else:
                raise AssertionError("verifier accepted corrupted post event or response")
            if correction:
                changed = copy.deepcopy(trace)
                acknowledgment = next(row["message"] for row in changed if row["direction"] == "backend"
                                      and row["message"].get("id") == 5 and "result" in row["message"])
                acknowledgment["id"] = 999
                try:
                    verify(hooks, requests, calls, fixture, success, thread, turn, cwd, transcript, True, changed)
                except AssertionError:
                    pass
                else:
                    raise AssertionError("verifier accepted an unacknowledged correction boundary")
                for violation in ["content", "envelope", "timestamp"]:
                    changed = copy.deepcopy(trace)
                    notification = next(row["message"] for row in changed
                                        if row["direction"] == "backend"
                                        and row["message"].get("method") == "item/completed"
                                        and row["message"]["params"]["item"]["type"] == "userMessage"
                                        and row["message"]["params"]["turnId"] == corrected_turn)
                    if violation == "content":
                        notification["params"]["item"]["content"][0]["text"] = "missing correction"
                    elif violation == "envelope":
                        notification["unqualifiedField"] = "extra framing"
                    else:
                        notification["emittedAtMs"] = "unbounded timestamp"
                    try:
                        verify(hooks, requests, calls, fixture, success, thread, turn, cwd, transcript, True, changed)
                    except AssertionError:
                        pass
                    else:
                        raise AssertionError(f"verifier accepted corrupted correction notification {violation}")
            return {"success": success, "correction": correction, "model_requests": len(requests), "events": len(hooks),
                    "correlation_violation_rejected": True, "event_violation_rejected": True}
    finally:
        if process is not None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            process.stdin.close()
            process.stdout.close()
        selector.close()
        server.shutdown()
        server.server_close()
        peer_thread.join(timeout=2)
        (root / "events.json").write_text(json.dumps(events, indent=2) + "\n")
        (root / "model-requests.json").write_text(json.dumps(requests, indent=2) + "\n")
        (root / "trace.json").write_text(json.dumps(trace, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    args = parser.parse_args()
    binary = args.codex.resolve(strict=True)
    fixture = json.loads(FIXTURE.read_text())
    with binary.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    assert digest == fixture["executable_sha256"], "binary differs from qualified pin"
    root = Path(tempfile.mkdtemp(prefix="demoncoder-codex-post-source-"))
    print(f"Evidence: {root}", flush=True)
    (root / "probe.py").write_bytes(Path(__file__).read_bytes())
    (root / "fixture.json").write_bytes(FIXTURE.read_bytes())
    cases = [run_case(binary, root / mode, fixture, mode == "success") for mode in ["success", "failure"]]
    cases.append(run_case(binary, root / "correction", fixture, True, correction=True))
    result = {"kind": "controlled-pinned-source", "executable_sha256": digest, "cases": cases}
    result["artifacts"] = [{"path": str(path.relative_to(root)),
                            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                           for path in sorted(root.glob("*/*.json*"))]
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
