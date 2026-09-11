#!/usr/bin/env python3
"""Qualify pinned Claude skill one-shot behavior using synthetic local peers.

This observes the actual source runtime, not DemonCoder production conformance.
No ambient credentials are inherited. Artifacts remain under --output.
"""

import argparse
import copy
import hashlib
import http.server
import json
import os
import selectors
import shlex
import signal
import subprocess
import threading
import time
import uuid
from dataclasses import asdict, dataclass
from pathlib import Path

FIXTURE = Path(__file__).parent / "fixtures/plugins/claude-once-source.json"
MAX_BYTES = 2 * 1024 * 1024
TOOL = "mcp__demoncoder__capture"
MARKER = "ONCE_PROBE_LOADED_MARKER"


@dataclass(frozen=True)
class ObserverProbe:
    first_line: bool = False
    rewake: bool = False
    idle: bool = False


def run(binary, base, name, once, exitcode, background, *, observer=None):
    root = base / ("claude-once-" + name + "-" + uuid.uuid4().hex[:8])
    root.mkdir(mode=0o700)
    (root / "probe.py").write_bytes(Path(__file__).read_bytes())
    home = root / "home"
    home.mkdir()
    work = root / "work"
    work.mkdir()
    skill = work / ".claude/skills/onceprobe"
    skill.mkdir(parents=True)
    log = root / "calls.jsonl"
    script = root / "capture.py"
    prelude = ""
    completion = ""
    if observer:
        if observer.first_line:
            prelude = (
                'print(json.dumps({"async":True,"asyncTimeout":100}),flush=True)\n'
            )
        completion = (
            'print("ASYNC_REWAKE_PROBE_MARKER",file=sys.stderr,flush=True)\n'
            if observer.idle
            else 'print(json.dumps({"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"DYNAMIC_ASYNC_CONTEXT_MARKER"}}),flush=True)\n'
        )
    script.write_text(
        "import json,sys,time,os\nfrom pathlib import Path\np=Path("
        + repr(str(log))
        + ')\nv=json.load(sys.stdin)\ndef emit(stage):\n with p.open("a") as f: f.write(json.dumps({"stage":stage,"time":time.monotonic(),"event":v,"pid":os.getpid()})+"\\n")\nemit("start")\n'
        + prelude
        + "time.sleep("
        + ("0.4" if background or observer else "0")
        + ")\n"
        + completion
        + 'emit("end")\nsys.exit('
        + str(exitcode)
        + ")\n"
    )
    skilltext = (
        "---\nname: onceprobe\ndescription: Synthetic qualification skill\nhooks:\n  PostToolUse:\n    - matcher: "
        + TOOL
        + "\n      hooks:\n        - type: command\n          command: "
        + json.dumps(shlex.join(["/usr/bin/python3", str(script)]))
        + "\n          once: "
        + str(once).lower()
        + "\n          async: "
        + str(background and not (observer and observer.first_line)).lower()
        + (
            "\n          asyncRewake: " + str(observer.rewake).lower()
            if observer
            else ""
        )
        + "\n---\nONCE_PROBE_LOADED_MARKER. Call the capture tool as instructed by the local fixture.\n"
    )
    (skill / "SKILL.md").write_text(skilltext)
    requests = []
    events = []
    trace = []
    errors = []
    calls = []
    results = []

    def logs():
        return (
            [json.loads(x) for x in log.read_text().splitlines()]
            if log.exists()
            else []
        )

    class Model(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            self.connection.settimeout(3)
            try:
                n = int(self.headers.get("Content-Length", "0"))
                assert 0 < n <= MAX_BYTES
                req = json.loads(self.rfile.read(n))
                requests.append(req)
                i = len(requests)
                assert i <= (3 if observer and observer.idle else 5)
                trace.append(
                    {"direction": "model", "time": time.monotonic(), "index": i}
                )
                if (
                    background
                    and i == 2
                    and name != "async-pending"
                    and not (observer and observer.idle)
                ):
                    deadline = time.monotonic() + 3
                    while (
                        not any(x["stage"] == "end" for x in logs())
                        and time.monotonic() < deadline
                    ):
                        time.sleep(0.02)
                    time.sleep(0.15)
                if i in ([1] if observer and observer.idle else [1, 2, 4]):
                    block = {
                        "type": "tool_use",
                        "id": "call_" + str(i),
                        "name": TOOL,
                        "input": {},
                    }
                    delta = {
                        "type": "input_json_delta",
                        "partial_json": json.dumps({"step": i}),
                    }
                    stop = "tool_use"
                else:
                    block = {"type": "text", "text": ""}
                    delta = {"type": "text_delta", "text": "done"}
                    stop = "end_turn"
                chunks = [
                    {
                        "type": "message_start",
                        "message": {
                            "id": "msg_" + str(i),
                            "type": "message",
                            "role": "assistant",
                            "model": "claude-sonnet-4-6",
                            "content": [],
                            "stop_reason": None,
                            "usage": {"input_tokens": 100, "output_tokens": 0},
                        },
                    },
                    {"type": "content_block_start", "index": 0, "content_block": block},
                    {"type": "content_block_delta", "index": 0, "delta": delta},
                    {"type": "content_block_stop", "index": 0},
                    {
                        "type": "message_delta",
                        "delta": {"stop_reason": stop, "stop_sequence": None},
                        "usage": {"output_tokens": 20},
                    },
                    {"type": "message_stop"},
                ]
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                for chunk in chunks:
                    self.wfile.write(
                        (
                            "event: "
                            + chunk["type"]
                            + "\ndata: "
                            + json.dumps(chunk)
                            + "\n\n"
                        ).encode()
                    )
                self.wfile.flush()
            except Exception as e:
                errors.append(repr(e))
                self.close_connection = True

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
    server.daemon_threads = True
    server.timeout = 3
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    env = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(home),
        "CLAUDE_CONFIG_DIR": str(home),
        "ANTHROPIC_API_KEY": "synthetic-key",
        "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{server.server_port}",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
        "DISABLE_TELEMETRY": "1",
        "DISABLE_ERROR_REPORTING": "1",
        "DISABLE_AUTOUPDATER": "1",
    }
    command = [
        str(binary),
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--replay-user-messages",
        "--include-partial-messages",
        "--model",
        "claude-sonnet-4-6",
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
        json.dumps(
            {"mcpServers": {"demoncoder": {"type": "sdk", "name": "demoncoder"}}}
        ),
        "--setting-sources",
        "project",
        "--permission-prompt-tool",
        "stdio",
    ]
    (root / "inputs.json").write_text(
        json.dumps(
            {
                "command": command,
                "environment": env,
                "skill": skilltext,
                "executable_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "once": once,
                "exit": exitcode,
                "async": background,
                **({"observer_probe": asdict(observer)} if observer else {}),
            },
            indent=2,
        )
    )
    proc = None
    sel = selectors.DefaultSelector()
    failure = None
    observed_until = None
    try:
        with (root / "stderr").open("wb") as stderr:
            proc = subprocess.Popen(
                command,
                cwd=work,
                env=env,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=stderr,
                start_new_session=True,
            )

            def send(m):
                if m.get("type") == "user":
                    m["uuid"] = str(uuid.uuid4())
                trace.append(
                    {"direction": "host", "time": time.monotonic(), "message": m}
                )
                proc.stdin.write((json.dumps(m) + "\n").encode())
                proc.stdin.flush()

            send(
                {
                    "type": "control_request",
                    "request_id": "initialize",
                    "request": {"subtype": "initialize", "hooks": {}, "skills": []},
                }
            )
            sel.register(proc.stdout, selectors.EVENT_READ)
            pending = b""
            deadline = time.monotonic() + 35
            received = 0
            while time.monotonic() < deadline and (
                len(results) < 2 or (observer and observer.idle)
            ):
                if not sel.select(0.1):
                    continue
                chunk = os.read(proc.stdout.fileno(), 65536)
                assert chunk, "closed before completion"
                received += len(chunk)
                assert received <= MAX_BYTES
                pending += chunk
                while b"\n" in pending:
                    line, pending = pending.split(b"\n", 1)
                    m = json.loads(line)
                    events.append(m)
                    trace.append(
                        {"direction": "backend", "time": time.monotonic(), "message": m}
                    )
                    if (
                        m.get("type") == "control_response"
                        and m["response"].get("request_id") == "initialize"
                    ):
                        assert m["response"]["subtype"] == "success"
                        send(
                            {
                                "type": "user",
                                "message": {
                                    "role": "user",
                                    "content": (
                                        "Perform synthetic captures."
                                        if name == "no-load"
                                        else "/onceprobe"
                                    ),
                                },
                            }
                        )
                    if m.get("type") == "result":
                        results.append(m)
                        assert not m.get("is_error"), m
                        if len(results) == 1 and observer and observer.idle:
                            deadline = min(deadline, time.monotonic() + 2)
                        elif len(results) == 1:
                            send(
                                {
                                    "type": "user",
                                    "message": {
                                        "role": "user",
                                        "content": (
                                            "Perform another synthetic capture."
                                            if name
                                            in ["success-no-reactivate", "no-load"]
                                            else "/onceprobe"
                                        ),
                                    },
                                }
                            )
                    if m.get("type") != "control_request":
                        continue
                    req = m["request"]
                    kind = req.get("subtype")
                    if kind == "can_use_tool":
                        answer = {"behavior": "allow", "updatedInput": req["input"]}
                    elif kind == "mcp_message":
                        rpc = req["message"]
                        method = rpc.get("method")
                        if method == "initialize":
                            result = {
                                "protocolVersion": rpc["params"]["protocolVersion"],
                                "capabilities": {"tools": {}},
                                "serverInfo": {"name": "demoncoder", "version": "1"},
                            }
                        elif method == "tools/list":
                            result = {
                                "tools": [
                                    {
                                        "name": "capture",
                                        "description": "Capture synthetic input",
                                        "inputSchema": {
                                            "type": "object",
                                            "additionalProperties": True,
                                        },
                                    }
                                ]
                            }
                        elif method == "tools/call":
                            calls.append(rpc)
                            result = {
                                "content": [{"type": "text", "text": "captured"}],
                                "isError": False,
                            }
                        elif method in [
                            "notifications/initialized",
                            "notifications/cancelled",
                            "ping",
                        ]:
                            result = {}
                        else:
                            raise AssertionError(method)
                        answer = {
                            "mcp_response": {
                                "jsonrpc": "2.0",
                                "id": rpc.get("id"),
                                "result": result,
                            }
                        }
                    else:
                        raise AssertionError(kind)
                    send(
                        {
                            "type": "control_response",
                            "response": {
                                "subtype": "success",
                                "request_id": m["request_id"],
                                "response": answer,
                            },
                        }
                    )
            observed_until = time.monotonic()
            if observer and observer.idle:
                assert (
                    len(results) in [1, 2]
                    and len(requests) in [2, 3]
                    and len(calls) == 1
                    and not errors
                ), (len(results), len(requests), len(calls), errors)
            else:
                assert (
                    len(results) == 2
                    and len(requests) == 5
                    and len(calls) == 3
                    and not errors
                ), (len(results), len(requests), len(calls), errors)
            if background:
                time.sleep(0.7)
    except Exception as e:
        failure = repr(e)
    finally:
        if proc:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait(timeout=5)
            proc.stdin.close()
            proc.stdout.close()
        sel.close()
        server.shutdown()
        server.server_close()
        thread.join(2)
        for filename, rows in [
            ("events.json", events),
            ("model-requests.json", requests),
            ("trace.json", trace),
        ]:
            (root / filename).write_text(json.dumps(rows, indent=2) + "\n")
    loaded = ["ONCE_PROBE_LOADED_MARKER" in json.dumps(r) for r in requests]
    result = {
        "case": name,
        "root": str(root),
        "failure": failure,
        "model_requests": len(requests),
        "tool_calls": len(calls),
        "result_count": len(results),
        "skill_loaded_per_request": loaded,
        "hook_log": logs(),
        "server_errors": errors,
        **({"observed_until": observed_until} if observer else {}),
    }
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(
        json.dumps({key: result[key] for key in ["case", "root", "failure"]}),
        flush=True,
    )
    return result


def verify(case, events, requests, trace, fixture):
    expected = fixture["cases"][case["case"]]["hook_steps"]
    assert case["failure"] is None and case["server_errors"] == []
    assert (case["model_requests"], case["tool_calls"], case["result_count"]) == (
        5,
        3,
        2,
    )
    assert len(requests) == 5
    loaded = [MARKER in json.dumps(request) for request in requests]
    assert loaded == case["skill_loaded_per_request"]
    assert loaded == ([False] * 5 if case["case"] == "no-load" else [True] * 5)
    results = [event for event in events if event.get("type") == "result"]
    assert len(results) == 2 and all(
        result.get("is_error") is False for result in results
    )
    assert results[0]["session_id"] == results[1]["session_id"]
    calls = [
        row
        for row in trace
        if row["direction"] == "backend"
        and row["message"].get("request", {}).get("message", {}).get("method")
        == "tools/call"
    ]
    assert [
        row["message"]["request"]["message"]["params"]["arguments"] for row in calls
    ] == [{"step": step} for step in [1, 2, 4]]
    for row, step in zip(calls, [1, 2, 4], strict=True):
        request = row["message"]["request"]
        assert request["server_name"] == "demoncoder"
        assert request["message"]["params"]["name"] == "capture"
        assert (
            request["message"]["params"]["_meta"]["claudecode/toolUseId"]
            == f"call_{step}"
        )
    prompts = [
        row["message"]["message"]["content"]
        for row in trace
        if row["direction"] == "host" and row["message"].get("type") == "user"
    ]
    assert prompts == [
        "Perform synthetic captures." if case["case"] == "no-load" else "/onceprobe",
        (
            "Perform another synthetic capture."
            if case["case"] in ["success-no-reactivate", "no-load"]
            else "/onceprobe"
        ),
    ]
    starts = [row for row in case["hook_log"] if row["stage"] == "start"]
    ends = [row for row in case["hook_log"] if row["stage"] == "end"]
    assert len(starts) + len(ends) == len(case["hook_log"])
    assert [row["event"]["tool_input"]["step"] for row in starts] == expected
    assert sorted(row["event"]["tool_use_id"] for row in starts) == sorted(
        row["event"]["tool_use_id"] for row in ends
    )
    for start in starts:
        event = start["event"]
        assert event["hook_event_name"] == "PostToolUse" and event["tool_name"] == TOOL
        assert event["tool_use_id"] == "call_" + str(event["tool_input"]["step"])
        assert event["session_id"] == results[0]["session_id"]
        assert event["tool_response"] == [{"type": "text", "text": "captured"}]
        end = next(
            row for row in ends if row["event"]["tool_use_id"] == event["tool_use_id"]
        )
        assert (
            end["event"] == event
            and end["pid"] == start["pid"]
            and end["time"] >= start["time"]
        )
    if case["case"].startswith("async"):
        second = calls[1]["time"]
        first_end = next(
            row["time"] for row in ends if row["event"]["tool_input"]["step"] == 1
        )
        if case["case"] == "async-pending":
            assert second < first_end
        else:
            assert second > first_end + 0.1


def verify_and_attack(case, fixture):
    root = Path(case["root"])
    log = root / "calls.jsonl"
    raw = (
        [json.loads(line) for line in log.read_text().splitlines()]
        if log.exists()
        else []
    )
    assert raw == case["hook_log"], "raw command evidence differs from summary"
    events, requests, trace = [
        json.loads((root / name).read_text())
        for name in ["events.json", "model-requests.json", "trace.json"]
    ]
    verify(case, events, requests, trace, fixture)
    mutations = []
    bad = copy.deepcopy(case)
    bad["tool_calls"] = 2
    mutations.append((bad, events, requests, trace))
    bad = copy.deepcopy(case)
    bad["skill_loaded_per_request"] = [
        not item for item in bad["skill_loaded_per_request"]
    ]
    mutations.append((bad, events, requests, trace))
    if case["hook_log"]:
        bad = copy.deepcopy(case)
        bad["hook_log"] = bad["hook_log"][1:]
        mutations.append((bad, events, requests, trace))
    bad_events = copy.deepcopy(events)
    next(item for item in bad_events if item.get("type") == "result")["is_error"] = True
    mutations.append((case, bad_events, requests, trace))
    bad_trace = copy.deepcopy(trace)
    next(
        row
        for row in bad_trace
        if row["direction"] == "host" and row["message"].get("type") == "user"
    )["message"]["message"]["content"] = "wrong prompt"
    mutations.append((case, events, requests, bad_trace))
    bad_trace = copy.deepcopy(trace)
    next(
        row
        for row in bad_trace
        if row["direction"] == "backend"
        and row["message"].get("request", {}).get("message", {}).get("method")
        == "tools/call"
    )["message"]["request"]["message"]["params"]["name"] = "wrong_tool"
    mutations.append((case, events, requests, bad_trace))
    bad_trace = copy.deepcopy(trace)
    next(
        row
        for row in bad_trace
        if row["direction"] == "backend"
        and row["message"].get("request", {}).get("message", {}).get("method")
        == "tools/call"
    )["message"]["request"]["message"]["params"]["arguments"] = {"step": 99}
    mutations.append((case, events, requests, bad_trace))
    for arguments in mutations:
        try:
            verify(*arguments, fixture)
        except AssertionError:
            continue
        raise AssertionError("verifier accepted corrupted evidence")
    paths = [
        "probe.py",
        "capture.py",
        "inputs.json",
        "events.json",
        "model-requests.json",
        "trace.json",
        "result.json",
        "stderr",
        "work/.claude/skills/onceprobe/SKILL.md",
    ]
    if (root / "calls.jsonl").exists():
        paths.append("calls.jsonl")
    return {
        "case": case["case"],
        "root": str(root),
        "hook_steps": fixture["cases"][case["case"]]["hook_steps"],
        "mutations_rejected": len(mutations),
        "artifacts": [
            {
                "path": path,
                "sha256": hashlib.sha256((root / path).read_bytes()).hexdigest(),
            }
            for path in paths
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claude", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if not __debug__:
        raise RuntimeError("Python optimization disables this fixture's assertions")
    fixture = json.loads(FIXTURE.read_text())
    binary = args.claude.resolve(strict=True)
    assert (
        hashlib.sha256(binary.read_bytes()).hexdigest() == fixture["executable_sha256"]
    ), "wrong executable pin"
    # Require a fresh destination so failed runs cannot reuse old success artifacts.
    base = args.output.resolve()
    base.mkdir(mode=0o700)
    reports = []
    for name, variant in fixture["cases"].items():
        case = run(
            binary, base, name, variant["once"], variant["exit_code"], variant["async"]
        )
        reports.append(verify_and_attack(case, fixture))
    assert (
        hashlib.sha256(binary.read_bytes()).hexdigest() == fixture["executable_sha256"]
    ), "executable changed during probe"
    result = {
        "kind": fixture["kind"],
        "backend": fixture["backend"],
        "executable_sha256": fixture["executable_sha256"],
        "probe_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "fixture_sha256": hashlib.sha256(FIXTURE.read_bytes()).hexdigest(),
        "cases": reports,
        "limits": fixture["limits"],
    }
    (base / "qualification.json").write_text(json.dumps(result, indent=2) + "\n")
    print(
        f"PASS: {len(reports)} source cases, {sum(row['mutations_rejected'] for row in reports)} corrupted-evidence rejections"
    )


if __name__ == "__main__":
    main()
