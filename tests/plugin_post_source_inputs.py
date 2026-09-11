#!/usr/bin/env python3
"""Check pinned Claude post-tool framing against real SDK exchanges with local peers.

This qualifies source behavior, not DemonCoder's production implementation.
Run with --claude /path/to/qualified/claude-2.1.267.
"""

import argparse
import copy
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import tempfile
import threading
import time
import uuid


FIXTURE = Path(__file__).parent / "fixtures/plugins/claude-post-source.json"
MAX_BYTES = 2 * 1024 * 1024
CORRECTION_MARKER = "[Plugin-origin fixture correction]"


def correction_prompt(fixture):
    prompt = (CORRECTION_MARKER + " The host interrupted the backend for this plugin correction, not a developer denial. "
            "Any backend cancellation placeholder describes the interruption, not the actual tool effect. "
            "Host tool completed with original evidence: "
            + json.dumps(fixture["original_content"])
            + ". Continue remaining work without repeating the completed tool.")
    content = fixture.get("correction_content")
    return prompt if content is None else [{"type": "text", "text": prompt}, *content]


def verify_correction(trace, requests, fixture):
    interrupts = [i for i, row in enumerate(trace) if row["direction"] == "host"
                  and row["message"].get("request_id") == "interrupt-post"]
    acknowledgments = [i for i, row in enumerate(trace) if row["direction"] == "backend"
                       and row["message"].get("type") == "control_response"
                       and row["message"]["response"].get("request_id") == "interrupt-post"
                       and row["message"]["response"]["subtype"] == "success"]
    results = [i for i, row in enumerate(trace) if row["direction"] == "backend"
               and row["message"].get("type") == "result"]
    followups = [i for i, row in enumerate(trace) if row["direction"] == "host"
                and row["message"].get("type") == "user"
                and row["message"]["message"]["content"] == correction_prompt(fixture)]
    models = [i for i, row in enumerate(trace) if row["direction"] == "model"]
    assert len(interrupts) == len(acknowledgments) == len(followups) == 1
    assert len(results) == len(models) == len(requests) == 2
    assert models[0] < interrupts[0] < acknowledgments[0] < followups[0] < models[1]
    assert interrupts[0] < results[0] < followups[0] < results[1]
    interrupted = trace[results[0]]["message"]
    assert interrupted["is_error"] is True and interrupted["terminal_reason"] == "aborted_tools"
    assert trace[results[1]]["message"]["is_error"] is False
    assert trace[results[1]]["message"]["session_id"] == interrupted["session_id"]
    sent = trace[followups[0]]["message"]
    echoes = [i for i, row in enumerate(trace) if row["direction"] == "backend"
              and row["message"].get("type") == "user" and row["message"].get("isReplay") is True
              and row["message"].get("uuid") == sent["uuid"]]
    # HTTP arrival and stdout reads are separate transports. The exact echo
    # acknowledges an already authorized request; it need not precede HTTP arrival.
    assert len(echoes) == 1 and followups[0] < echoes[0] < results[1]
    response_starts = [i for i, row in enumerate(trace) if row["direction"] == "backend"
                       and row["message"].get("type") == "stream_event"
                       and row["message"].get("event", {}).get("type") == "message_start"
                       and i > followups[0]]
    assert len(response_starts) == 1 and echoes[0] < response_starts[0]
    echoed = trace[echoes[0]]["message"]
    assert echoed["session_id"] == interrupted["session_id"]
    assert echoed["message"] == sent["message"] and echoed["parent_tool_use_id"] is None
    assert sent["session_id"] == interrupted["session_id"] and sent["parent_tool_use_id"] is None
    assert set(echoed) == set(sent) | {"isReplay", "timestamp"}
    assert isinstance(echoed["timestamp"], str)
    assert re.fullmatch(r"(?:[0-9]{4}|[+-][0-9]{6})-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}Z",
                        echoed["timestamp"]), "unexpected source replay timestamp shape"
    post_request = next(row["message"]["request_id"] for row in trace if row["direction"] == "backend"
                        and row["message"].get("request", {}).get("callback_id") == "PostToolUse")
    assert not any(row["direction"] == "host" and row["message"].get("type") == "control_response"
                   and row["message"]["response"].get("request_id") == post_request for row in trace)
    expected = correction_prompt(fixture)
    expected_blocks = [{"type": "text", "text": expected}] if isinstance(expected, str) else expected
    assert any(message.get("role") == "user" and (
        message.get("content") == expected
        or isinstance(message.get("content"), list) and all(
            any(all(actual.get(key) == value for key, value in block.items())
                for actual in message["content"] if isinstance(actual, dict))
            for block in expected_blocks)) for message in requests[1]["messages"])


def verify(events, requests, fixture, mode, trace):
    controls = [event["request"] for event in events
                if event.get("type") == "control_request"]
    callbacks = [request for request in controls if request.get("subtype") == "hook_callback"]
    expected_event = "PostToolUseFailure" if mode == "failure" else "PostToolUse"
    expected_events = ["PreToolUse", expected_event]
    if mode == "replacement-object":
        expected_events.append("PostToolUseFailure")
    assert [item["input"]["hook_event_name"] for item in callbacks] == expected_events
    pre, post = [item["input"] for item in callbacks[:2]]
    for item in callbacks:
        assert item["tool_use_id"] == fixture["tool_use_id"]
        assert item["input"]["tool_use_id"] == fixture["tool_use_id"]
        assert item["input"]["tool_name"] == fixture["tool_name"]
        assert item["input"]["tool_input"] == fixture["arguments"]
    for field in ["session_id", "transcript_path", "cwd"]:
        assert isinstance(pre[field], str) and pre[field]
        assert pre[field] == post[field]
    permissions = [item for item in controls if item.get("subtype") == "can_use_tool"]
    assert len(permissions) == 1
    assert permissions[0]["tool_use_id"] == fixture["tool_use_id"]
    calls = [item for item in controls if item.get("message", {}).get("method") == "tools/call"]
    assert len(calls) == 1
    assert calls[0]["message"]["params"]["_meta"][fixture["metadata_key"]] == fixture["tool_use_id"]
    assert calls[0]["message"]["params"]["arguments"] == fixture["arguments"]
    assert controls.index(callbacks[0]) < controls.index(permissions[0]) < controls.index(calls[0])
    assert controls.index(calls[0]) < controls.index(callbacks[1])
    if mode == "failure":
        assert post["error"] == fixture["failure_error"] and post["is_interrupt"] is False
        assert "tool_response" not in post
    else:
        assert post["tool_response"] == fixture["original_content"]
    if mode == "correction":
        verify_correction(trace, requests, fixture)
        return
    assert len(requests) == 2
    results = [part for message in requests[-1]["messages"]
               if isinstance(message.get("content"), list) for part in message["content"]
               if part.get("type") == "tool_result"]
    assert len(results) == 1 and results[0]["tool_use_id"] == fixture["tool_use_id"]
    if mode == "replacement-object":
        assert fixture["invalid_replacement_error_fragment"] in callbacks[-1]["input"]["error"]
        assert results[0]["is_error"] is True
        assert isinstance(results[0]["content"], str)
        assert fixture["invalid_replacement_error_fragment"] in results[0]["content"]
    elif mode == "failure":
        assert results[0]["is_error"] is True
        assert isinstance(results[0]["content"], str)
        assert results[0]["content"].partition("\n\n<system-reminder>")[0] == fixture["failure_error"]
    elif mode == "replacement-string":
        assert results[0].get("is_error") is not True
        assert isinstance(results[0]["content"], str)
        assert results[0]["content"].partition("\n\n<system-reminder>")[0] == fixture["replacement_cases"][mode]
    else:
        assert results[0].get("is_error") is not True
        expected = fixture.get("observed_replacements", {}).get(mode)
        if expected is None:
            expected = fixture["replacement_cases"].get(mode)
            # The source uses JavaScript truthiness; empty arrays are truthy.
            if expected is None or expected is False or expected == 0 or expected == "":
                expected = fixture["original_content"]
        if isinstance(expected, str):
            assert isinstance(results[0]["content"], str)
            assert results[0]["content"].partition("\n\n<system-reminder>")[0] == expected
        else:
            assert results[0]["content"] == expected
    context = fixture.get("additional_context", {}).get(mode)
    if context:
        assert any(context in block.get("text", "")
                   for message in requests[-1]["messages"]
                   if isinstance(message.get("content"), list)
                   for block in message["content"] if block.get("type") == "text")


def run_case(binary, root, fixture, mode, *, lifecycle=None):
    if lifecycle not in [None, "pass", "submit-deny", "stop-correct"]:
        raise ValueError("unknown lifecycle source case")
    (root / "home").mkdir(parents=True)
    (root / "work").mkdir()
    requests, errors, events, trace, request_metadata = [], [], [], [], []

    class Model(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            try:
                length = int(self.headers.get("Content-Length", "0"))
                assert 0 < length <= MAX_BYTES and len(requests) < 2
                request = json.loads(self.rfile.read(length))
                # Retain only protocol capability headers, never credentials.
                request_metadata.append({"path": self.path, "headers": {
                    name: self.headers.get(name) for name in
                    ["anthropic-version", "anthropic-beta", "content-type"]
                    if self.headers.get(name) is not None}})
                trace.append({"direction": "model", "message": request})
                requests.append(request)
                if len(requests) == 1 and lifecycle is None:
                    block = {"type": "tool_use", "id": fixture["tool_use_id"],
                             "name": fixture["tool_name"], "input": {}}
                    delta = {"type": "input_json_delta", "partial_json": json.dumps(fixture["arguments"])}
                    stop = "tool_use"
                else:
                    block = {"type": "text", "text": ""}
                    delta = {"type": "text_delta", "text": "done"}
                    stop = "end_turn"
                chunks = [
                    {"type": "message_start", "message": {"id": "msg_post_source", "type": "message",
                     "role": "assistant", "model": "claude-sonnet-4-6", "content": [], "stop_reason": None,
                     "usage": {"input_tokens": 100, "output_tokens": 0}}},
                    {"type": "content_block_start", "index": 0, "content_block": block},
                    {"type": "content_block_delta", "index": 0, "delta": delta},
                    {"type": "content_block_stop", "index": 0},
                    {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None},
                     "usage": {"output_tokens": 20}},
                    {"type": "message_stop"},
                ]
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                for chunk in chunks:
                    self.wfile.write(("event: " + chunk["type"] + "\ndata: " + json.dumps(chunk) + "\n\n").encode())
                self.wfile.flush()
            except Exception as error:
                errors.append(repr(error))
                self.close_connection = True

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(root / "home"),
                   "CLAUDE_CONFIG_DIR": str(root / "home"), "ANTHROPIC_API_KEY": "synthetic-key",
                   "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{server.server_port}",
                   "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "DISABLE_TELEMETRY": "1",
                   "DISABLE_ERROR_REPORTING": "1", "DISABLE_AUTOUPDATER": "1"}
    command = [str(binary), "-p", "--input-format", "stream-json", "--output-format", "stream-json",
               "--verbose", "--replay-user-messages", "--include-partial-messages", "--model", "claude-sonnet-4-6", "--tools", "",
               "--strict-mcp-config", "--mcp-config", json.dumps({"mcpServers": {
                   "demoncoder": {"type": "sdk", "name": "demoncoder"}}}),
               "--setting-sources", "", "--permission-prompt-tool", "stdio"]
    if lifecycle is not None:
        (root / "inputs.json").write_text(json.dumps({"command": command, "environment": environment,
                                                     "case": lifecycle}, indent=2) + "\n")
    process = None
    selector = selectors.DefaultSelector()
    try:
        with (root / "stderr").open("wb") as stderr:
            process = subprocess.Popen(command, cwd=root / "work", env=environment,
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=stderr, start_new_session=True)

            def send(message):
                if message.get("type") == "user":
                    message["uuid"] = str(uuid.uuid4())
                trace.append({"direction": "host", "message": message})
                process.stdin.write((json.dumps(message) + "\n").encode())
                process.stdin.flush()

            hooks = {event: [{"matcher": fixture["tool_name"], "hookCallbackIds": [event], "timeout": 5}]
                     for event in ["PreToolUse", "PostToolUse", "PostToolUseFailure"]}
            if lifecycle is not None:
                hooks = {event: [{"hookCallbackIds": [event], "timeout": 5}]
                         for event in ["UserPromptSubmit", "Stop"]}
            send({"type": "control_request", "request_id": "initialize",
                  "request": {"subtype": "initialize", "hooks": hooks, "skills": []}})
            selector.register(process.stdout, selectors.EVENT_READ)
            pending = b""
            received = 0
            deadline = time.monotonic() + 30
            complete = False
            interrupted_result = None
            interrupt_ack = False
            correction_sent = False
            interrupt_started = None
            while time.monotonic() < deadline and not complete:
                if not selector.select(.1):
                    continue
                chunk = os.read(process.stdout.fileno(), 65536)
                assert chunk, "backend closed before completion"
                received += len(chunk)
                assert received <= MAX_BYTES, "backend output exceeded bound"
                pending += chunk
                while b"\n" in pending:
                    line, pending = pending.split(b"\n", 1)
                    message = json.loads(line)
                    trace.append({"direction": "backend", "message": message})
                    events.append(message)
                    if message.get("type") == "control_response" and message["response"].get("request_id") == "initialize":
                        assert message["response"]["subtype"] == "success"
                        send({"type": "user", "message": {"role": "user", "content": "Perform the fixture operation once."}})
                    if message.get("type") == "result":
                        if mode == "correction" and not correction_sent:
                            interrupted_result = message
                        else:
                            assert message.get("is_error") is False
                            complete = True
                            break
                    if (message.get("type") == "control_response"
                            and message["response"].get("request_id") == "interrupt-post"):
                        assert message["response"]["subtype"] == "success"
                        interrupt_ack = True
                    if interrupted_result is not None and interrupt_ack and not correction_sent:
                        assert len(requests) == 1, "backend crossed held post callback"
                        assert time.monotonic() - interrupt_started < 3, "interrupt waited for hook timeout"
                        send({"type": "user", "message": {"role": "user", "content": correction_prompt(fixture)},
                              "parent_tool_use_id": None, "session_id": interrupted_result["session_id"]})
                        correction_sent = True
                    if message.get("type") != "control_request":
                        continue
                    request = message["request"]
                    kind = request.get("subtype")
                    if kind == "can_use_tool":
                        answer = {"behavior": "allow", "updatedInput": request["input"]}
                    elif kind == "hook_callback":
                        if mode == "correction" and request["callback_id"] == "PostToolUse":
                            interrupt_started = time.monotonic()
                            send({"type": "control_request", "request_id": "interrupt-post",
                                  "request": {"subtype": "interrupt"}})
                            continue
                        answer = {}
                        if (lifecycle == "submit-deny" and request["callback_id"] == "UserPromptSubmit"):
                            answer = {"decision": "block", "reason": "SOURCE_SUBMIT_DENY"}
                        elif (lifecycle == "stop-correct" and request["callback_id"] == "Stop"
                              and len(requests) == 1):
                            answer = {"decision": "block", "reason": "SOURCE_STOP_CORRECTION"}
                        if mode in fixture["replacement_cases"] and request["callback_id"] == "PostToolUse":
                            answer = {"hookSpecificOutput": {"hookEventName": "PostToolUse",
                                      "updatedMCPToolOutput": fixture["replacement_cases"][mode]}}
                            context = fixture.get("additional_context", {}).get(mode)
                            if context:
                                answer["hookSpecificOutput"]["additionalContext"] = context
                    elif kind == "mcp_message":
                        rpc = request["message"]
                        method = rpc.get("method")
                        if method == "initialize":
                            result = {"protocolVersion": rpc["params"]["protocolVersion"],
                                      "capabilities": {"tools": {}}, "serverInfo": {"name": "demoncoder", "version": "1"}}
                        elif method == "tools/list":
                            result = {"tools": [{"name": "capture", "description": "Capture synthetic input",
                                      "inputSchema": {"type": "object", "additionalProperties": True}}]}
                        elif method == "tools/call":
                            result = {"content": fixture["original_content"], "isError": mode == "failure"}
                        elif method in ["notifications/initialized", "notifications/cancelled", "ping"]:
                            result = {}
                        else:
                            raise AssertionError(f"unexpected MCP method: {method}")
                        answer = {"mcp_response": {"jsonrpc": "2.0", "id": rpc.get("id"), "result": result}}
                    else:
                        raise AssertionError(f"unexpected control request: {kind}")
                    send({"type": "control_response", "response": {"subtype": "success",
                          "request_id": message["request_id"], "response": answer}})
            assert complete and not errors, f"incomplete local exchange: {errors}"
            if lifecycle is not None:
                return {"case": lifecycle, "model_requests": len(requests), "errors": errors}
            verify(events, requests, fixture, mode, trace)
            changed = copy.deepcopy(events)
            call = next(item["request"]["message"] for item in changed
                        if item.get("request", {}).get("message", {}).get("method") == "tools/call")
            call["params"]["_meta"][fixture["metadata_key"]] = "wrong-source-id"
            try:
                verify(changed, requests, fixture, mode, trace)
            except AssertionError:
                pass
            else:
                raise AssertionError("verifier accepted corrupted source correlation")
            changed = copy.deepcopy(events)
            post = [item["request"]["input"] for item in changed
                    if item.get("request", {}).get("subtype") == "hook_callback"][1]
            if mode == "failure":
                post["error"] = {"content": fixture["original_content"]}
            else:
                post["tool_response"] = {"content": fixture["original_content"], "isError": False}
            try:
                verify(changed, requests, fixture, mode, trace)
            except AssertionError:
                pass
            else:
                raise AssertionError("verifier accepted corrupted source result framing")
            if mode != "correction":
                changed = copy.deepcopy(requests)
                result = next(block for message in changed[-1]["messages"]
                              if isinstance(message.get("content"), list)
                              for block in message["content"] if block.get("type") == "tool_result")
                result["content"] = [{"type": "text", "text": "corrupted downstream result"}]
                try:
                    verify(events, changed, fixture, mode, trace)
                except AssertionError:
                    pass
                else:
                    raise AssertionError("verifier accepted changed downstream result content")
            context = fixture.get("additional_context", {}).get(mode)
            if context:
                changed = copy.deepcopy(requests)
                for message in changed[-1]["messages"]:
                    if isinstance(message.get("content"), list):
                        message["content"] = [block for block in message["content"]
                                              if context not in block.get("text", "")]
                try:
                    verify(events, changed, fixture, mode, trace)
                except AssertionError:
                    pass
                else:
                    raise AssertionError("verifier accepted missing plugin attribution")
            if mode == "correction":
                changed = copy.deepcopy(requests)
                expected = correction_prompt(fixture)
                expected_blocks = [{"type": "text", "text": expected}] if isinstance(expected, str) else expected
                # Remove one semantic block from the actual next model request;
                # the echoed host message must not substitute for model delivery.
                victim = expected_blocks[-1]
                removed = False
                for message in changed[-1]["messages"]:
                    if isinstance(message.get("content"), list):
                        kept = []
                        for block in message["content"]:
                            if not removed and all(block.get(key) == value for key, value in victim.items()):
                                removed = True
                            else:
                                kept.append(block)
                        message["content"] = kept
                assert removed, "corrective semantic block missing before mutation"
                try:
                    verify(events, changed, fixture, mode, trace)
                except AssertionError:
                    pass
                else:
                    raise AssertionError("verifier accepted missing corrective model content")
                changed = copy.deepcopy(trace)
                acknowledgment = next(row["message"]["response"] for row in changed
                                      if row["direction"] == "backend"
                                      and row["message"].get("type") == "control_response"
                                      and row["message"]["response"].get("request_id") == "interrupt-post")
                acknowledgment["request_id"] = "unrelated-interrupt"
                try:
                    verify(events, requests, fixture, mode, changed)
                except AssertionError:
                    pass
                else:
                    raise AssertionError("verifier accepted an unacknowledged correction boundary")
                changed = copy.deepcopy(trace)
                echo = next(row["message"] for row in changed if row["direction"] == "backend"
                            and row["message"].get("isReplay") is True
                            and row["message"].get("message", {}).get("content") == correction_prompt(fixture))
                echo["uuid"] = "00000000-0000-4000-8000-000000000000"
                try:
                    verify(events, requests, fixture, mode, changed)
                except AssertionError:
                    pass
                else:
                    raise AssertionError("verifier accepted an unrelated user-message acknowledgment")
                for field, value in [("timestamp", "x" * 28), ("unexpected_envelope_field", True)]:
                    changed = copy.deepcopy(trace)
                    echo = next(row["message"] for row in changed if row["direction"] == "backend"
                                and row["message"].get("isReplay") is True
                                and row["message"].get("message", {}).get("content") == correction_prompt(fixture))
                    echo[field] = value
                    try:
                        verify(events, requests, fixture, mode, changed)
                    except AssertionError:
                        pass
                    else:
                        raise AssertionError("verifier accepted changed replay envelope shape")
            return {"mode": mode, "model_requests": len(requests),
                    "correlation_violation_rejected": True, "framing_violation_rejected": True,
                    "content_violation_rejected": True,
                    "attribution_violation_rejected": bool(context),
                    "replay_envelope_violation_rejected": mode == "correction"}
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
        thread.join(timeout=2)
        (root / "events.json").write_text(json.dumps(events, indent=2) + "\n")
        (root / "model-requests.json").write_text(json.dumps(requests, indent=2) + "\n")
        (root / "request-metadata.json").write_text(json.dumps(request_metadata, indent=2) + "\n")
        (root / "trace.json").write_text(json.dumps(trace, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claude", type=Path, required=True)
    args = parser.parse_args()
    binary = args.claude.resolve(strict=True)
    fixture = json.loads(FIXTURE.read_text())
    with binary.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    assert digest == fixture["executable_sha256"], "binary differs from qualified pin"
    root = Path(tempfile.mkdtemp(prefix="demoncoder-post-source-"))
    print(f"Evidence: {root}", flush=True)
    (root / "probe.py").write_bytes(Path(__file__).read_bytes())
    (root / "fixture.json").write_bytes(FIXTURE.read_bytes())
    modes = ["success", "failure", *fixture["replacement_cases"], "correction"]
    cases = [run_case(binary, root / mode, fixture, mode) for mode in modes]
    for name, content in fixture.get("correction_cases", {}).items():
        variant = copy.deepcopy(fixture)
        variant["correction_content"] = content
        case = run_case(binary, root / name, variant, "correction")
        case["variant"] = name
        cases.append(case)
    result = {"kind": "controlled-pinned-source", "executable_sha256": digest, "cases": cases}
    result["artifacts"] = [{"path": str(path.relative_to(root)),
                            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                           for path in sorted(root.glob("*/*.json"))]
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
