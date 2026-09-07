#!/usr/bin/env python3
"""Actual terminal delegation and isolated effects across all four connections."""
import argparse
import http.server
import json
import os
import subprocess
import tempfile
import threading
import time
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from verification_workflow import App
from terminal_session import Provider
from tool_cycle_fixture import sse_call
from subagent_backend_fixture import assignment, calls_for

ADAPTERS = ("openai-api", "anthropic-api", "codex", "claude")


class AgentProvider(Provider):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests.append(body)
        history = body["input"] if self.path == "/responses" else body["messages"]
        strings = [item["content"] for item in history if isinstance(item.get("content"), str)]
        child_prompt = next((text for text in strings if text.startswith("You are assigned child agent")), None)
        last = history[-1]
        call = None
        text = "Parent available for independent work."
        if not body.get("tools"):
            if "outside-access Oracle" in strings[-1]:
                self.server.oracles.append(body)
                text = json.dumps({"decision": "allow", "reason": "Disposable synthetic fixture only"})
            else:
                self.server.reviews.append(body)
                text = json.dumps(self.server.verdict)
        elif child_prompt:
            request = assignment(child_prompt)
            assert body["model"] == "child-" + request["connection"], body
            calls = calls_for(request)
            completed = sum(item.get("type") == "function_call_output" for item in history)
            completed += sum(part.get("type") == "tool_result" for item in history if isinstance(item.get("content"), list) for part in item["content"])
            if completed < len(calls):
                call = {**calls[completed], "id": f"child-{completed}"}
            text = "Child finished; inspect original results."
        elif isinstance(last.get("content"), str) and last["content"].startswith("assign "):
            request = {"connection": last["content"].split()[1], "objective": "write child result", "owned_paths": ["greeting"], "context": ""}
            request.update(self.server.assignment)
            call = {"id": f"delegate-{len(self.server.requests)}", "name": "delegate", "arguments": request}
        elif isinstance(last.get("content"), str) and last["content"].startswith("parent write"):
            call = {"id": f"parent-write-{len(self.server.requests)}", "name": "write", "arguments": {"path": "parent-budget-escape", "content": "unbudgeted effect"}}
        events = sse_call(self.path, call) if call else ([
            {"type": "message_start", "message": {"usage": {"input_tokens": 11}}},
            {"type": "content_block_delta", "delta": {"type": "text_delta", "text": text}},
            {"type": "message_delta", "usage": {"output_tokens": 8}},
            {"type": "message_stop"},
        ] if self.path == "/messages" else [
            {"type": "response.output_text.delta", "delta": text},
            {"type": "response.completed", "response": {"output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}], "usage": {"input_tokens": 11, "output_tokens": 8}}},
        ])
        data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        try:
            self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            pass


def server_fixture():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), AgentProvider)
    server.requests, server.reviews, server.oracles = [], [], []
    server.assignment = {}
    server.verdict = {"verdict": "clear", "findings": [], "explanation": "Actual child greeting and successful executed check agree."}
    server.extra_config = ""
    for adapter in ADAPTERS:
        server.extra_config += f'\n[connections.{adapter}]\nadapter="{adapter}"\nmodel="child-{adapter}"\n'
        if adapter in ("openai-api", "anthropic-api"):
            route = "responses" if adapter == "openai-api" else "messages"
            server.extra_config += f'endpoint="http://127.0.0.1:{server.server_port}/{route}"\n'
        else:
            server.extra_config += f'binary={json.dumps(str(Path(__file__).with_name("subagent_backend_fixture.py")))}\n'
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def repository(directory):
    project = Path(directory) / "project"
    project.mkdir()
    subprocess.run(["git", "init", "-q", str(project)], check=True)
    (project / "greeting").write_text("committed\n")
    subprocess.run(["git", "-C", str(project), "add", "."], check=True)
    subprocess.run(["git", "-C", str(project), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture baseline"], check=True)
    (project / "greeting").write_text("developer dirty edit\n")
    (project / "untracked").write_bytes(b"untracked baseline\x00\xff")
    return project


def launch(directory, server, extra=()):
    flags = [value for adapter in ADAPTERS for value in ("--agent-connection", adapter)]
    return App(directory, server, [*flags, "--check", "test -s greeting", "--reviewer", "worker", *extra])


def agent(app, identifier=1):
    return next(item for item in app.record()[1]["agents"] if item["id"] == identifier)


def stopped(app, identifier=1):
    app.wait_for(lambda: any(item["id"] == identifier and item["status"] not in ("preparing", "running", "validating", "integrating") for item in app.record()[1]["agents"]), timeout=20)
    record = agent(app, identifier)
    assert record["status"] == "stopped", record
    return record


def delegate(app, server, adapter, calls=None, owned=("greeting",)):
    server.assignment = {"owned_paths": list(owned), "context": json.dumps({"fixture_calls": calls}) if calls is not None else ""}
    app.send("assign " + adapter)
    records = app.record()[1]["agents"]
    assert records, app.events()
    return records[-1]["id"]


def assignments(server):
    for adapter in ADAPTERS:
        with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-") as directory:
            project = repository(directory)
            app = launch(directory, server)
            try:
                record = stopped(app, delegate(app, server, adapter))
                root = Path(record["worktree"]["root"])
                assert root != project and (root / ".git").is_file()
                assert (root / "greeting").read_text() == "child result\n"
                assert (project / "greeting").read_text() == "developer dirty edit\n"
                assert (root / "untracked").read_bytes() == (project / "untracked").read_bytes()
                assert record["request"]["connection"] == adapter and record["identity"]["adapter"] == adapter
                assert record["origin"] == "parent_agent"
                assert record["commands"] == ["test -s greeting"] and record["reviewer"]
                results = [event["result"] for event in record["activity"] if event["type"] == "tool_finished"]
                assert [result["tool"] for result in results] == ["read", "write", "edit", "bash"]
                assert all(result["success"] for result in results), results
                assert results[0]["output"] == "developer dirty edit\n"
                assert any(event["type"] == "agent_activity" for event in app.events())
                if adapter in ("codex", "claude"):
                    assert app.record()[1]["backend_invocations"] == 1
                app.send("/agents")
                app.send("/agent 1")
                app.send(f"/delegate {adapter} greeting terminal assignment")
                assert stopped(app, 2)["origin"] == "developer"
            finally:
                app.close()


def confinement(server):
    for adapter in ADAPTERS:
        with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-home-") as directory:
            project = repository(directory)
            canary = Path(directory) / "home-canary"
            canary.write_text("preserve synthetic home\n")
            calls = [
                {"name": "write", "arguments": {"path": str(canary), "content": "destroyed"}},
                {"name": "write", "arguments": {"path": "../escape", "content": "destroyed"}},
                {"name": "write", "arguments": {"path": ".git", "content": "destroyed"}},
                {"name": "bash", "arguments": {"command": f"test -d {directory} && rm -rf {directory}"}},
                {"name": "bash", "arguments": {"command": f"mv {directory} /tmp/stolen-synthetic-home"}},
                {"name": "read", "arguments": {"path": str(canary)}},
                {"name": "delegate", "arguments": {}},
                {"name": "write", "arguments": {"path": "greeting", "content": "safe child effect\n"}},
            ]
            app = launch(directory, server, ["--yolo"])
            try:
                record = stopped(app, delegate(app, server, adapter, calls))
                results = [item["result"] for item in record["activity"] if item["type"] == "tool_finished"]
                assert len(results) == len(calls), record
                assert all(not result["success"] for result in results[:-1]), results
                assert results[-1]["success"], results
                assert canary.read_text() == "preserve synthetic home\n"
                assert (project / "greeting").read_text() == "developer dirty edit\n"
                assert record["identity"]["strict_worktree"] and not record["identity"]["unrestricted"]
            finally:
                app.close()


def integration(server):
    for adapter in ADAPTERS:
        with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-merge-") as directory:
            project = repository(directory)
            app = launch(directory, server)
            try:
                stopped(app, delegate(app, server, adapter))
                app.send("/agent-integrate 1")
                assert agent(app)["status"] == "stopped", agent(app)
                app.send("/agent-validate 1")
                app.wait_for(lambda: agent(app)["status"] != "validating", timeout=20)
                record = agent(app)
                assert record["status"] == "ready" and record["review"]["clear"], record
                assert record["checks"][0]["success"]
                (project / "unrelated").write_text("parent independent edit\n")
                app.send("/agent-integrate 1")
                app.wait_for(lambda: agent(app)["status"] != "integrating", timeout=20)
                assert agent(app)["status"] == "integrated", agent(app)
                assert (project / "greeting").read_text() == "child result\n"
                assert (project / "unrelated").read_text() == "parent independent edit\n"
            finally:
                app.close()


def integration_gates(server):
    for mode in ("failed-check", "review-findings", "stale-child", "parent-conflict", "unowned", "parent-acceptance"):
        with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-gate-") as directory:
            project = repository(directory)
            app = launch(directory, server)
            try:
                calls = None
                if mode == "failed-check":
                    calls = [{"name": "write", "arguments": {"path": "greeting", "content": ""}}]
                elif mode == "unowned":
                    calls = [{"name": "write", "arguments": {"path": "unowned", "content": "must not merge"}}]
                if mode == "parent-acceptance":
                    app.send("/task preserve the starting greeting")
                    app.send("/verify")
                    app.send("/review")
                    app.send("/accept")
                    assert app.record()[1]["task"]["accepted"]
                    app.send("/delegate anthropic-api greeting write child result")
                    stopped(app)
                else:
                    stopped(app, delegate(app, server, "anthropic-api", calls))
                if mode == "review-findings":
                    server.verdict = {"verdict": "findings", "findings": ["Fixture deliberate finding"], "explanation": "Do not integrate this candidate."}
                app.send("/agent-validate 1")
                app.wait_for(lambda: agent(app)["status"] != "validating", timeout=20)
                root = Path(agent(app)["worktree"]["root"])
                if mode in ("failed-check", "review-findings", "unowned"):
                    assert agent(app)["status"] != "ready", agent(app)
                    app.send("/agent-integrate 1")
                    assert (project / "greeting").read_text() == "developer dirty edit\n"
                    assert not (project / "unowned").exists()
                    if mode == "failed-check":
                        assert not agent(app)["checks"][0]["success"]
                    elif mode == "review-findings":
                        assert agent(app)["review"]["findings"] == ["Fixture deliberate finding"]
                    continue
                assert agent(app)["status"] == "ready", agent(app)
                if mode == "stale-child":
                    (root / "greeting").write_text("changed after validation\n")
                elif mode == "parent-conflict":
                    (project / "greeting").write_text("concurrent parent edit\n")
                app.send("/agent-integrate 1")
                app.wait_for(lambda: agent(app)["status"] != "integrating", timeout=20)
                if mode == "stale-child":
                    assert agent(app)["status"] != "integrated"
                    assert (project / "greeting").read_text() == "developer dirty edit\n"
                elif mode == "parent-conflict":
                    assert agent(app)["status"] == "uncertain", agent(app)
                    assert app.record()[1]["recovery_pending"]
                    assert (project / "greeting").read_text() == "concurrent parent edit\n"
                else:
                    assert agent(app)["status"] == "integrated", agent(app)
                    task = app.record()[1]["task"]
                    assert task["accepted"] is None and not task["checks"] and task["review"] is None
                    assert task["check_history"] and task["review_history"], "integration lost original parent evidence"
                    app.send("/task-status")
                    assert not app.state()["accepted"] and app.state()["verification"] == "unverified"
            finally:
                server.verdict = {"verdict": "clear", "findings": [], "explanation": "Actual child greeting and successful executed check agree."}
                app.close()


def cancellation(server):
    for adapter in ADAPTERS:
        with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-cancel-") as directory:
            repository(directory)
            app = launch(directory, server)
            try:
                for mode in ("individual", "parent", "shutdown"):
                    calls = [{"name": "bash", "arguments": {"command": "while :; do printf x >> heartbeat; sleep .02; done"}}]
                    identifier = delegate(app, server, adapter, calls, ("heartbeat",))
                    app.wait_for(lambda: agent(app, identifier)["worktree"] is not None)
                    heartbeat = Path(agent(app, identifier)["worktree"]["root"]) / "heartbeat"
                    app.wait_for(lambda: heartbeat.exists() and heartbeat.stat().st_size >= 3)
                    app.send("parent independent work while child runs")
                    assert agent(app, identifier)["status"] == "running"
                    started = time.monotonic()
                    if mode == "individual":
                        app.send(f"/agent-cancel {identifier}")
                    elif mode == "parent":
                        os.write(app.master, b"\x1b")
                        app.wait_for(lambda: agent(app, identifier)["status"] != "running")
                    else:
                        app.close()
                    assert time.monotonic() - started < 2, mode
                    before = heartbeat.read_bytes()
                    time.sleep(.15)
                    assert heartbeat.read_bytes() == before, mode
                    if mode == "shutdown":
                        app = None
                        break
            finally:
                if app is not None:
                    app.close()


def budgets(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-budget-") as directory:
        repository(directory)
        app = launch(directory, server, ["--task-tool-calls", "2"])
        try:
            calls = [
                {"name": "write", "arguments": {"path": "greeting", "content": "admitted\n"}},
                {"name": "write", "arguments": {"path": "forbidden", "content": "must not appear"}},
            ]
            stopped(app, delegate(app, server, "anthropic-api", calls, ("greeting", "forbidden")))
            record = agent(app)
            results = [item["result"] for item in record["activity"] if item["type"] == "tool_finished"]
            assert results[0]["success"] and not results[1]["success"], results
            assert not (Path(record["worktree"]["root"]) / "forbidden").exists()
            assert app.record()[1]["allocation"]["tool_calls"] == 2
            app.send("/agent-cancel 1")
            app.send("/abandon")
            app.send("parent write after abandon")
            assert not (app.workspace / "parent-budget-escape").exists(), "abandon removed shared tool admissions"
            assert app.record()[1]["allocation"]["tool_calls"] == 2
        finally:
            app.close()


def concurrency(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-concurrency-") as directory:
        repository(directory)
        app = launch(directory, server)
        try:
            stopped(app, delegate(app, server, "anthropic-api"))
            app.send("/agent-validate 1")
            app.wait_for(lambda: agent(app)["status"] == "ready")
            calls = [{"name": "bash", "arguments": {"command": "while :; do sleep .02; done"}}]
            for _ in range(2):
                identifier = delegate(app, server, "anthropic-api", calls)
                app.wait_for(lambda: agent(app, identifier)["status"] == "running")
            app.send("/agent-integrate 1")
            assert agent(app)["status"] == "ready", "integration exceeded child concurrency limit"
            assert all(event["active"] <= event["active_limit"] for event in app.events() if event["type"] == "agent_allocation")
            assert (app.workspace / "greeting").read_text() == "developer dirty edit\n"
        finally:
            app.close()


def model_backend_and_deadline_limits(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-model-limit-") as directory:
        repository(directory)
        app = launch(directory, server, ["--task-model-calls", "1"])
        try:
            app.send("/delegate anthropic-api greeting use bounded calls")
            app.wait_for(lambda: agent(app)["status"] not in ("preparing", "running"))
            record = agent(app)
            assert record["status"] == "failed", record
            assert app.record()[1]["allocation"]["model_calls"] == 1
            assert (Path(record["worktree"]["root"]) / "greeting").read_text() == "developer dirty edit\n"
        finally:
            app.close()
    with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-backend-limit-") as directory:
        repository(directory)
        app = launch(directory, server, ["--agent-backend-turns", "1"])
        try:
            app.send("/delegate codex greeting bounded subscription")
            stopped(app)
            app.send("/delegate claude greeting no invocation remains")
            app.wait_for(lambda: agent(app, 2)["status"] not in ("preparing", "running"))
            record = agent(app, 2)
            assert record["status"] == "failed", record
            assert app.record()[1]["backend_invocations"] == 1
            assert (Path(record["worktree"]["root"]) / "greeting").read_text() == "developer dirty edit\n"
            assert app.record()[1]["allocation"]["usage"]["unknown_cost"]
        finally:
            app.close()
    for adapter in ADAPTERS:
        with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-deadline-") as directory:
            repository(directory)
            app = launch(directory, server, ["--task-seconds", "2"])
            try:
                calls = [{"name": "bash", "arguments": {"command": "while :; do printf x >> heartbeat; sleep .02; done"}}]
                identifier = delegate(app, server, adapter, calls, ("heartbeat",))
                app.wait_for(lambda: agent(app, identifier)["worktree"] is not None)
                heartbeat = Path(agent(app, identifier)["worktree"]["root"]) / "heartbeat"
                app.wait_for(lambda: heartbeat.exists())
                app.wait_for(lambda: agent(app, identifier)["status"] not in ("preparing", "running"), timeout=4)
                before = heartbeat.read_bytes()
                time.sleep(.1)
                assert heartbeat.read_bytes() == before, "deadline left child effects running"
                app.send("parent write after deadline")
                assert not (app.workspace / "parent-budget-escape").exists()
            finally:
                app.close()


def recovery(server):
    for adapter in ADAPTERS:
        with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-recovery-") as directory:
            repository(directory)
            app = launch(directory, server)
            try:
                calls = [{"name": "bash", "arguments": {"command": "while :; do printf x >> heartbeat; sleep .02; done"}}]
                identifier = delegate(app, server, adapter, calls, ("heartbeat",))
                app.wait_for(lambda: agent(app, identifier)["worktree"] is not None)
                original = agent(app, identifier)
                heartbeat = Path(original["worktree"]["root"]) / "heartbeat"
                app.wait_for(lambda: heartbeat.exists() and heartbeat.stat().st_size >= 3)
                path, saved = app.record()
                app.process.kill()
                app.process.wait(timeout=2)
                app.close()
                app = None
                time.sleep(.15)
                stopped_bytes = heartbeat.read_bytes()
                requests = len(server.requests)
                app = launch(directory, server, ["--resume", str(path)])
                app.wait_for(lambda: any(event["type"] == "agent_state" for event in app.events()))
                restored = agent(app, identifier)
                assert restored["status"] == "uncertain", restored
                assert restored["worktree"] == original["worktree"]
                assert restored["request"] == original["request"]
                assert app.record()[1]["allocation"]["tool_calls"] == saved["allocation"]["tool_calls"]
                assert app.record()[1]["backend_invocations"] == saved["backend_invocations"]
                assert len(server.requests) == requests, "resume replayed model work"
                assert heartbeat.read_bytes() == stopped_bytes, "orphaned child continued effects"
                app.send(f"/agent {identifier}")
                app.send(f"/agent-reconcile {identifier} inspected the stopped heartbeat and retained worktree")
                assert agent(app, identifier)["status"] == "failed"
                assert not agent(app, identifier)["completed"]
                assert heartbeat.read_bytes() == stopped_bytes
            finally:
                if app is not None:
                    app.close()


def interrupted_transition(server, phase):
    with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-transition-") as directory:
        project = repository(directory)
        app = launch(directory, server)
        try:
            if phase == "integrating":
                stopped(app, delegate(app, server, "anthropic-api"))
                app.send("/agent-validate 1")
                app.wait_for(lambda: agent(app)["status"] == "ready")
                command = "/agent-integrate 1"
            else:
                command = "/delegate anthropic-api greeting prepare isolated worktree"
            path, _ = app.record()
            observed = []
            def kill_at_intent():
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline and app.process.poll() is None:
                    payload = json.loads((path / "state.json").read_text())["payload"]
                    if payload["agents"] and payload["agents"][0]["status"] == phase:
                        observed.append(payload["agents"][0])
                        app.process.kill()
                        return
                    time.sleep(.0001)
            watcher = threading.Thread(target=kill_at_intent)
            watcher.start()
            os.write(app.master, command.encode() + b"\r")
            watcher.join(timeout=6)
            assert observed, f"did not observe durable {phase} intent"
            app.process.wait(timeout=2)
            app.close()
            app = None
            time.sleep(.15)
            parent_after_crash = (project / "greeting").read_bytes()
            requests = len(server.requests)
            app = launch(directory, server, ["--resume", str(path)])
            app.wait_for(lambda: any(event["type"] == "agent_state" for event in app.events()))
            restored = agent(app)
            assert restored["status"] == "uncertain", restored
            assert restored["planned_root"] == observed[0]["planned_root"]
            assert restored["integration"] == observed[0]["integration"]
            assert app.record()[1]["recovery_pending"]
            assert len(server.requests) == requests, "interruption resumed model work automatically"
            assert (project / "greeting").read_bytes() == parent_after_crash, "resume replayed integration"
            if phase == "integrating":
                assert restored["integration"] and restored["review"]["clear"]
                assert restored["checks"][0]["success"]
            else:
                assert not restored["completed"]
        finally:
            if app is not None:
                app.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--requirement", required=True)
    args = parser.parse_args()
    server = server_fixture()
    try:
        if args.requirement in ("SUB-001", "SUB-003"):
            assignments(server)
        elif args.requirement == "SUB-002":
            confinement(server)
        elif args.requirement == "SUB-005":
            integration(server)
            integration_gates(server)
        elif args.requirement == "SUB-004":
            cancellation(server)
        elif args.requirement == "SUB-006":
            budgets(server)
            concurrency(server)
            model_backend_and_deadline_limits(server)
        elif args.requirement == "SUB-007":
            recovery(server)
            interrupted_transition(server, "preparing")
            interrupted_transition(server, "integrating")
        else:
            raise AssertionError(f"production coverage not implemented for {args.requirement}")
    finally:
        server.shutdown()
        server.server_close()
    print(f"{args.requirement}: production terminal cases passed")


if __name__ == "__main__":
    main()
