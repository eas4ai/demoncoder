#!/usr/bin/env python3
"""Task acceptance exercises through the production terminal and HTTP adapter."""
import argparse
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import signal
import sys
import tempfile
import termios
import threading
import time

sys.dont_write_bytecode = True
from terminal_session import BINARY, Provider, until
from tool_cycle_fixture import sse_call
from cancellation import descendants, identity


class WorkflowProvider(Provider):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        history = body["input"] if self.path == "/responses" else body["messages"]
        self.server.requests.append(body)
        if not body.get("tools") and getattr(self.server, "delay_review", False):
            time.sleep(2)
        if body.get("tools") and getattr(self.server, "delay_worker", False):
            time.sleep(2)
        if getattr(self.server, "delay_tool_result", False) and (history[-1].get("type") == "function_call_output" or isinstance(history[-1].get("content"), list)):
            time.sleep(2)
        if not body.get("tools"):
            prompt = history[-1]["content"]
            if "outside-access Oracle" in prompt:
                self.server.oracles.append(body)
                text = json.dumps({"decision":"allow", "reason":"Scoped disposable fixture effect"})
            else:
                self.server.reviews.append(body)
                text = json.dumps(self.server.verdict)
            call = self.server.review_tool
        else:
            last = history[-1].get("content")
            if isinstance(last, str):
                self.server.received.append(last)
            text = "Worker stopped; inspect verification before accepting."
            call = None
            if self.server.worker_tool and isinstance(last, str):
                call = {**self.server.worker_tool, "id":f"worker-{len(self.server.requests)}"}
            if self.server.edit_work and isinstance(last, str):
                corrected = last.startswith("Correct this task")
                call = {"id": f"write-{len(self.server.requests)}", "name": "write", "arguments": {
                    "path": "greeting", "content": "corrected\n" if corrected else "wrong\n"}}
        text_events = [
            {"type": "message_start", "message": {"usage": {"input_tokens": 11}}},
            {"type": "content_block_delta", "delta": {"type": "text_delta", "text": text}},
            {"type": "message_delta", "usage": {"output_tokens": 8}},
            {"type": "message_stop"},
        ] if self.path == "/messages" else [
            {"type":"response.output_text.delta", "delta":text},
            {"type":"response.completed", "response":{"output":[{"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":text}]}], "usage":{"input_tokens":11,"output_tokens":8}}},
        ]
        events = sse_call(self.path, call) if call else text_events
        data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        try:
            self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            pass  # The interrupted-response fixture deliberately kills its client.


class App:
    def __init__(self, directory, server, extra=(), expect_start=True, home_workspace=False):
        self.root = Path(directory)
        self.workspace = self.root if home_workspace else self.root / "project"
        self.workspace.mkdir(exist_ok=True)
        self.config = self.root / "settings.toml"
        adapter = getattr(server, "adapter", "anthropic-api")
        route = "responses" if adapter == "openai-api" else "messages"
        self.config.write_text(
            'onboarding_complete=true\ndefault_connection="worker"\n'
            f'[connections.worker]\nadapter="{adapter}"\nmodel="fixture-model"\n'
            + (f'endpoint="http://127.0.0.1:{server.server_port}/{route}"\n' if adapter in ("openai-api", "anthropic-api") else "")
        )
        if adapter in ("codex", "claude"):
            with self.config.open("a") as stream:
                stream.write(f'binary={json.dumps(str(Path(__file__).with_name("backend_fixture.py")))}\n')
        if "--yolo" in extra:
            with self.config.open("a") as stream:
                stream.write('\n[oracle]\nconnection="worker"\n')
        if getattr(server, "extra_config", ""):
            with self.config.open("a") as stream:
                stream.write(server.extra_config)
        self.config.chmod(0o600)
        self.log = self.root / f"events-{time.time_ns()}.jsonl"
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
        env = {"PATH": "/usr/bin:/bin", "HOME": str(self.root),
               "TERM": "xterm-256color", "LANG": "C.UTF-8",
               "ANTHROPIC_API_KEY": "synthetic-anthropic-key", "OPENAI_API_KEY":"synthetic-openai-key"}
        self.process = subprocess.Popen(
            [str(BINARY), "--trust-workspace", "--workspace", str(self.workspace),
             "--config", str(self.config), "--event-log", str(self.log), *extra],
            stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        self.output = bytearray()
        if expect_start:
            try:
                until(self.master, self.process, self.output, b"Prompt")
            except AssertionError as error:
                self.close()
                raise AssertionError(f"fixture startup failed: {self.output.decode(errors='replace')}") from error
        else:
            try:
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    if select.select([self.master], [], [], .02)[0]:
                        try:
                            self.output.extend(os.read(self.master, 65536))
                        except OSError:
                            break
                    if self.process.poll() is not None:
                        break
                assert self.process.wait(timeout=1) != 0, "unsafe startup was accepted"
            except BaseException:
                self.close()
                raise

    def events(self):
        if not self.log.exists():
            return []
        return [json.loads(line)["event"] for line in self.log.read_text().splitlines(keepends=True)
                if line.endswith("\n")]

    def send(self, prompt):
        count = sum(e["type"] == "turn_finished" for e in self.events())
        os.write(self.master, prompt.encode() + b"\r")
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.03)[0]:
                self.output.extend(os.read(self.master, 65536))
            if sum(e["type"] == "turn_finished" for e in self.events()) > count:
                return
            assert self.process.poll() is None, "app exited during task command"
        raise AssertionError(f"task command did not finish: {prompt}")

    def close(self):
        try:
            if self.process.poll() is None:
                os.write(self.master, b"\x11")
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait()
                    raise AssertionError("application failed to close")
        finally:
            if self.master is not None:
                os.close(self.master)
                self.master = None

    def state(self):
        return [e for e in self.events() if e["type"] == "task_state"][-1]

    def wait_for(self, predicate, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return
            assert self.process.poll() is None, "application stopped before checkpoint"
            if select.select([self.master], [], [], 0.03)[0]:
                self.output.extend(os.read(self.master, 65536))
        raise AssertionError("runtime checkpoint timed out")

    def record(self):
        path = Path([e["path"] for e in self.events() if e["type"] == "session_record"][0])
        return path, json.loads((path / "state.json").read_text())["payload"]


def acceptance_without_checks(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        app = App(directory, server)
        try:
            app.send("/task change greeting")
            app.send("/accept")
            states = [e for e in app.events() if e["type"] == "task_state"]
            assert states, "actual terminal has no separate task acceptance state"
            assert states[-1]["verification"] == "unverified", states[-1]
            assert states[-1]["accepted"] is False, states[-1]
            errors = [e["message"] for e in app.events() if e["type"] == "error"]
            assert any("checks" in e.lower() for e in errors), errors
            assert not any("/accept" in p for p in server.received), "acceptance went to model"
        finally:
            app.close()


def failed_corrected_and_stale(server):
    server.edit_work = True
    server.verdict = {"verdict": "clear", "findings": [], "explanation": "Actual greeting and executed assertion agree."}
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        app = App(directory, server, ["--check", 'test "$(cat greeting)" = corrected', "--reviewer", "worker"])
        try:
            (app.workspace / "greeting").write_text("original\n")
            app.send("/task change greeting to corrected")
            assert (app.workspace / "greeting").read_text() == "wrong\n"
            app.send("/verify")
            assert app.state()["verification"] == "failed", app.state()
            app.send("/accept")
            assert not app.state()["accepted"]
            app.send("/correct")
            assert (app.workspace / "greeting").read_text() == "corrected\n"
            app.send("/verify")
            assert app.state()["verification"] == "passed", app.state()
            app.send("/accept")
            assert not app.state()["accepted"], "checks alone accepted work"
            app.send("/review")
            assert app.state()["review"] == "clear", app.state()
            request = server.reviews[-1]
            evidence = request["messages"][-1]["content"]
            assert "original" in evidence and "corrected" in evidence
            assert "test" in evidence and "false" in evidence, "original failed check lost"
            assert not request["tools"], "reviewer received tools"
            (app.workspace / "new-untracked").write_text("unreviewed addition\n")
            app.send("/accept")
            assert not app.state()["accepted"], "stale review accepted changed workspace"
            app.send("/verify")
            app.send("/review")
            app.send("/accept")
            assert app.state()["accepted"], app.state()
            outcomes = [e["result"] for e in app.events() if e["type"] == "tool_finished" and e["result"]["call_id"].startswith("verify-")]
            assert outcomes[0]["success"] is False and outcomes[-1]["success"] is True
        finally:
            app.close()
            server.edit_work = False


def verification_confinement_and_cancel(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        root = Path(directory)
        outside = root / "outside-canary"
        outside.write_text("unchanged")
        check = f'printf changed > {outside}'
        app = App(directory, server, ["--check", check, "--check", 'test -z "$ANTHROPIC_API_KEY" && ! cat "$HOME/.demoncoder/private-fixture"'])
        (root / ".demoncoder/private-fixture").write_text("synthetic-private-value")
        try:
            app.send("/task inspect without outside effects")
            app.send("/verify")
            assert outside.read_text() == "unchanged"
            assert app.state()["verification"] == "failed"
            results = [e["result"] for e in app.events() if e["type"] == "tool_finished"]
            assert results[0]["success"] is False and results[1]["success"] is True, results
            assert "synthetic-private-value" not in app.log.read_text()
            _, record = app.record()
            assert len(record["task"]["checks"]) == 2
            assert record["task"]["checks"][0]["command"] == check
        finally:
            app.close()
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        app = App(directory, server, ["--check", '(while :; do printf x >> heartbeat; sleep .03; done) & wait'])
        heartbeat = app.workspace / "heartbeat"
        try:
            app.send("/task exercise cancellable verification")
            os.write(app.master, b"/verify\r")
            app.wait_for(lambda: heartbeat.exists() and heartbeat.stat().st_size >= 2)
            started = time.monotonic()
            os.write(app.master, b"\x1b")
            app.wait_for(lambda: any(e["type"] == "turn_finished" and e["status"] == "cancelled" for e in app.events()), timeout=2)
            assert time.monotonic() - started < 2
            size = heartbeat.stat().st_size
            time.sleep(.15)
            assert heartbeat.stat().st_size == size, "verification descendant survived cancellation"
            assert not app.state()["accepted"]
            _, record = app.record()
            assert record["recovery_pending"], "uncertain verification lost on cancellation"
        finally:
            app.close()


def verification_snapshot_failure_retains_result(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-verification-capture-") as directory:
        command = "printf retained-check-output; truncate -s 9000000 generated-output"
        app = App(directory, server, ["--check", command])
        try:
            app.send("/task retain verification when workspace capture fails")
            (app.workspace / "input-after-task").write_text("actual verification input")
            app.send("/verify")
            path, record = app.record()
            task = record["task"]
            operation = next(o for o in record["operations"] if o.get("call") and o["call"]["id"].startswith("verify-"))
            assert operation["phase"] == "verification", operation
            attribution = operation["verification"]
            assert attribution["task_id"] == task["id"]
            assert attribution["generation"] == task["verification_generation"]
            assert attribution["snapshot"] != task["baseline"]["digest"], "verification attributed to task baseline"
            assert operation["result"]["success"] is True
            assert operation["result"]["output"] == "retained-check-output"
            receipt = task["checks"][0]
            assert receipt["snapshot"] == attribution["snapshot"]
            assert receipt["command"] == command and receipt["exit_code"] == 0
            assert not receipt["success"] and "retained-check-output" in receipt["output"]
            assert "capture failed" in receipt["output"]
            assert task["accepted"] is None
        finally:
            app.close()
        (Path(directory) / "project/generated-output").unlink()
        app = App(directory, server, ["--resume", str(path)])
        try:
            _, restored = app.record()
            assert restored["task"]["checks"] == task["checks"]
            assert not (app.workspace / "generated-output").exists(), "completed verification was replayed"
            app.send("/accept")
            assert not app.state()["accepted"]
        finally:
            app.close()


def interrupted_verification_attribution(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-verification-interrupted-") as directory:
        app = App(directory, server, ["--check", "printf x >> check-counter; sleep 30"])
        try:
            app.send("/task retain interrupted verification attribution")
            (app.workspace / "input-after-task").write_text("actual verification input")
            os.write(app.master, b"/verify\r")
            app.wait_for(lambda: (app.workspace / "check-counter").exists())
            path, record = app.record()
            operation = next(o for o in record["operations"] if o.get("call") and o["call"]["id"].startswith("verify-"))
            assert operation["phase"] == "verification"
            assert operation["verification"]["task_id"] == record["task"]["id"]
            assert operation["verification"]["generation"] == record["task"]["verification_generation"]
            assert operation["verification"]["snapshot"] != record["task"]["baseline"]["digest"]
            assert not operation["complete"]
            app.process.kill()
            app.process.wait(timeout=3)
        finally:
            app.close()
        app = App(directory, server, ["--resume", str(path)])
        try:
            _, restored = app.record()
            assert restored["recovery_pending"]
            assert operation in restored["operations"]
            app.send("/verify")
            assert (app.workspace / "check-counter").read_text() == "x"
            assert not restored["task"]["accepted"]
        finally:
            app.close()


def reviewer_negative_cases(server):
    for verdict, tool in [("malformed verdict", None), ({"verdict":"blocked", "findings":[], "explanation":"Missing a behavior check"}, None),
                          ({"verdict":"clear", "findings":["contradiction"], "explanation":"invalid"}, None),
                          ({"verdict":"clear", "findings":[], "explanation":"unused"}, {"id":"review-write", "name":"write", "arguments":{"path":"reviewer-effect", "content":"forbidden"}})]:
        with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
            app = App(directory, server, ["--check", "true", "--reviewer", "worker"])
            server.verdict, server.review_tool = verdict, tool
            try:
                app.send("/task review real evidence")
                app.send("/verify")
                app.send("/review")
                app.send("/accept")
                assert not app.state()["accepted"], (verdict, tool)
                assert not (app.workspace / "reviewer-effect").exists()
            finally:
                app.close()
                server.review_tool = None
    for content in [b"\x00binary changed", b"text" * 300_000]:
        with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
            app = App(directory, server, ["--check", "true", "--reviewer", "worker"])
            try:
                app.send("/task inspect all required evidence")
                (app.workspace / "changed").write_bytes(content)
                app.send("/verify")
                before = len(server.reviews)
                app.send("/review")
                assert len(server.reviews) == before, "incomplete evidence reached reviewer"
                assert not app.state()["accepted"]
            finally:
                app.close()


def bounded_correction(server):
    server.verdict = {"verdict":"findings", "findings":["Required behavior is still absent"], "explanation":"Fixture retains a concrete finding"}
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        app = App(directory, server, ["--check", "false", "--reviewer", "worker"])
        try:
            app.send("/task correct within two rounds")
            app.send("/verify")
            app.send("/review")
            for _ in range(2):
                before = len(server.reviews)
                app.send("/correct")
                assert len(server.reviews) == before + 1, "correction skipped follow-up review"
                assert app.state()["verification"] == "failed"
            before = len(server.requests)
            app.send("/correct")
            app.send("keep trying")
            assert len(server.requests) == before, "correction or prompt reset the allowance"
            _, record = app.record()
            assert record["task"]["corrections"] == 2
            assert record["task"]["review"]["findings"]
            assert len(record["task"]["check_history"]) == 2
        finally:
            app.close()


def interrupted_reverification(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-reverification-") as directory:
        app = App(directory, server, ["--check", "if test -f hang; then sleep 30; else false; fi", "--correction-rounds", "0"])
        try:
            app.send("/task keep the correction limit after interrupted verification")
            app.send("/verify")
            assert app.state()["verification"] == "failed"
            (app.workspace / "hang").touch()
            count = sum(e["type"] == "tool_started" for e in app.events())
            finished = sum(e["type"] == "turn_finished" for e in app.events())
            os.write(app.master, b"/verify\r")
            app.wait_for(lambda: sum(e["type"] == "tool_started" for e in app.events()) > count)
            os.write(app.master, b"\x1b")
            app.wait_for(lambda: sum(e["type"] == "turn_finished" for e in app.events()) > finished)
            app.send("/reconcile inspected cancelled check; only the deliberate hang marker was added")
            count = len(server.requests)
            app.send("continue modifying this task")
            app.send("/correct")
            assert len(server.requests) == count, "cancelled reverification bypassed correction allowance"
            _, record = app.record()
            assert record["task"]["check_history"]
            assert record["task"]["corrections"] == 0
        finally:
            app.close()


def interrupted_review(server):
    server.verdict = {"verdict":"clear", "findings":[], "explanation":"Delayed fixture"}
    for cancel in (True, False):
        with tempfile.TemporaryDirectory(prefix="demoncoder-review-interruption-") as directory:
            flags = ["--check", "true", "--reviewer", "worker"]
            if not cancel:
                flags += ["--task-seconds", "1"]
            app = App(directory, server, flags)
            try:
                app.send("/task bound the reviewer phase")
                app.send("/verify")
                server.delay_review = True
                before = len(server.requests)
                if cancel:
                    finished = sum(e["type"] == "turn_finished" for e in app.events())
                    os.write(app.master, b"/review\r")
                    app.wait_for(lambda: len(server.requests) > before)
                    os.write(app.master, b"\x1b")
                    app.wait_for(lambda: sum(e["type"] == "turn_finished" for e in app.events()) > finished, timeout=2)
                    assert any(e["type"] == "turn_finished" and e["status"] == "cancelled" for e in app.events())
                else:
                    app.send("/review")
                    assert any("deadline exhausted" in e.get("message", "") for e in app.events())
                app.send("/accept")
                assert not app.state()["accepted"]
                _, record = app.record()
                assert record["task"]["review"] is None
                assert record["allocation"]["model_calls"] == 2
            finally:
                server.delay_review = False
                app.close()


def cumulative_allocations(server):
    for cap in ("--task-token-limit", "--task-cost-limit"):
        with tempfile.TemporaryDirectory(prefix="demoncoder-hard-cap-") as directory:
            before = len(server.requests)
            app = App(directory, server, [cap, "1"], expect_start=False)
            try:
                assert b"cannot be enforced" in app.output, app.output
                assert len(server.requests) == before
            finally:
                app.close()
    server.verdict = {"verdict":"clear", "findings":[], "explanation":"Fixture checks passed"}
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        flags = ["--check", "true", "--reviewer", "worker", "--task-model-calls", "1", "--task-tool-calls", "1"]
        app = App(directory, server, flags)
        try:
            app.send("/task retain cumulative limits")
            app.send("/verify")
            count = len(server.requests)
            app.send("/review")
            assert len(server.requests) == count, "review bypassed model allowance"
            app.send("/verify")
            assert app.state()["verification"] == "failed", "verification bypassed tool allowance"
            app.send("/task reset the same work")
            path, record = app.record()
            assert record["allocation"]["model_calls"] == 1
            assert record["allocation"]["tool_calls"] == 1
            assert record["allocation"]["usage"]["reported_input"] == 11
            assert record["allocation"]["usage"]["unknown_cost"]
            assert not record["allocation"]["usage"]["unknown_input"]
        finally:
            app.close()
        app = App(directory, server, ["--resume", str(path), "--reviewer", "worker", "--task-model-calls", "100"])
        try:
            count = len(server.requests)
            app.send("/review")
            assert len(server.requests) == count, "restart reset model allocation"
            _, recovered = app.record()
            assert recovered["allocation"]["limits"]["model_calls"] == 1
            assert recovered["allocation"]["deadline_ms"] == record["allocation"]["deadline_ms"]
        finally:
            app.close()
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        app = App(directory, server, ["--check", '(while :; do printf x >> heartbeat; sleep .03; done) & wait', "--task-seconds", "1"])
        try:
            app.send("/task share one deadline with verification")
            app.send("/verify")
            heartbeat = app.workspace / "heartbeat"
            size = heartbeat.stat().st_size
            time.sleep(.15)
            assert heartbeat.stat().st_size == size, "deadline left verification descendant alive"
            assert any("deadline exhausted" in e.get("message", "") for e in app.events())
            assert not app.state()["accepted"]
        finally:
            app.close()
    server.worker_tool = {"name":"bash", "arguments":{"command":"printf guarded > guarded"}}
    for limit in (1, 2):
        with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
            app = App(directory, server, ["--yolo", "--task-model-calls", str(limit)])
            try:
                before = len(server.oracles)
                app.send("/task create the guarded fixture file")
                assert (app.workspace / "guarded").exists() == (limit == 2)
                assert len(server.oracles) - before == limit - 1, "Oracle did not consume shared admission"
                _, record = app.record()
                assert record["allocation"]["model_calls"] == limit
                observed = sum(e.get("input") or 0 for e in app.events() if e["type"] in ("usage", "oracle_usage"))
                assert record["allocation"]["usage"]["reported_input"] == observed, record["allocation"]["usage"]
            finally:
                app.close()
    server.worker_tool = None


def recovery_cases(server):
    server.verdict = {"verdict":"clear", "findings":[], "explanation":"Actual fixture checked"}
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        flags = ["--check", "true", "--reviewer", "worker"]
        app = App(directory, server, flags)
        original = "remember this completed task across a restart"
        try:
            app.send("/task " + original)
            app.send("/verify")
            app.send("/review")
            app.send("/accept")
            assert app.state()["accepted"]
            path, before = app.record()
        finally:
            app.close()

        count = len(server.requests)
        app = App(directory, server, [*flags, "--resume", str(path)])
        try:
            app.send("/task-status")
            assert app.state()["accepted"], "accepted decision was forgotten"
            assert len(server.requests) == count, "restart repeated completed model work"
            assert any(e["type"] == "retained_message" and original in e["text"] for e in app.events())
            app.send("/task continue after the accepted work")
            history = json.dumps(server.requests[-1].get("messages", server.requests[-1].get("input")))
            assert original in history, "native conversation was not restored"
            _, record = app.record()
            assert record["archived"][0]["task"]["accepted"] == before["task"]["accepted"]
            assert record["archived"][0]["allocation"]["model_calls"] == before["allocation"]["model_calls"]
            assert record["archived"][0]["allocation"]["usage"] == before["allocation"]["usage"]
        finally:
            app.close()
    for access in ([], ["--yolo"]):
        with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
            server.worker_tool = {"name":"bash", "arguments":{"command":"printf x >> counter; sleep 30"}}
            app = App(directory, server, access)
            owned = {}
            try:
                os.write(app.master, b"/task mutate only once before interruption\r")
                app.wait_for(lambda: (app.workspace / "counter").exists())
                path, before = app.record()
                owned = descendants(app.process.pid)
                app.process.kill()
                app.process.wait(timeout=3)
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline and any(identity(pid) == birth for pid, birth in owned.items() if birth):
                    time.sleep(.02)
                alive = [pid for pid, birth in owned.items() if birth and identity(pid) == birth]
                assert not alive, f"crash left owned processes active: {alive}"
            finally:
                app.close()
                for pid, birth in owned.items():
                    if birth and identity(pid) == birth:
                        os.kill(pid, signal.SIGKILL)
                server.worker_tool = None
            count = len(server.requests)
            app = App(directory, server, [*access, "--resume", str(path)])
            try:
                app.send("try continuing without inspecting")
                assert len(server.requests) == count, "uncertain work was resumed without reconciliation"
                assert (app.workspace / "counter").read_text() == "x", "interrupted tool was replayed"
                _, interrupted = app.record()
                assert interrupted["recovery_pending"]
                assert interrupted["allocation"]["model_calls"] == before["allocation"]["model_calls"]
                app.send("/reconcile inspected counter; one write occurred and no process remains")
                app.send("continue with the inspected result")
                assert len(server.requests) == count + 1
                history = json.dumps(server.requests[-1].get("messages", server.requests[-1].get("input")))
                assert "not replayed" in history and "counter" in history
                assert (app.workspace / "counter").read_text() == "x"
                _, reconciled = app.record()
                assert not reconciled["recovery_pending"]
                assert reconciled["decisions"]
            finally:
                app.close()
            count = len(server.requests)
            app = App(directory, server, [*access, "--resume", str(path)])
            try:
                app.send("continue after the recorded inspection")
                assert len(server.requests) == count + 1, "restart asked again for answered reconciliation"
            finally:
                app.close()
    with tempfile.TemporaryDirectory(prefix="demoncoder-workflow-") as directory:
        app = App(directory, server, ["--check", "true", "--reviewer", "worker"])
        try:
            app.send("/task watch for changes while closed")
            app.send("/verify")
            app.send("/review")
            path, _ = app.record()
        finally:
            app.close()
        (Path(directory) / "project/changed-while-closed").write_text("developer change")
        app = App(directory, server, ["--resume", str(path), "--reviewer", "worker"])
        try:
            app.send("/accept")
            assert not app.state()["accepted"], "restart accepted evidence from before a workspace change"
            _, record = app.record()
            assert record["recovery_pending"]
        finally:
            app.close()


def recovery_refusals(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-recovery-refusals-") as directory:
        app = App(directory, server, ["--reviewer", "worker"])
        try:
            app.send("/task retain the original reviewer")
            path, _ = app.record()
            count = len(server.requests)
            competing = App(directory, server, ["--resume", str(path), "--reviewer", "worker"], expect_start=False)
            try:
                assert b"already open" in competing.output, competing.output
                assert len(server.requests) == count
            finally:
                competing.close()
        finally:
            app.close()
        changed = App(directory, server, ["--resume", str(path)], expect_start=False)
        try:
            assert b"original reviewer" in changed.output, changed.output
            assert len(server.requests) == count
        finally:
            changed.close()
        (path / "state.json").write_text('{"private fixture truncated')
        broken = App(directory, server, ["--resume", str(path), "--reviewer", "worker"], expect_start=False)
        try:
            assert b"private fixture truncated" not in broken.output
            assert len(server.requests) == count
        finally:
            broken.close()
    with tempfile.TemporaryDirectory(prefix="demoncoder-persistence-failure-") as directory:
        app = App(directory, server)
        try:
            app.send("/task refuse effects without a durable admission")
            path, _ = app.record()
            count = len(server.requests)
            (path / "state.json").chmod(0o400)
            app.send("continue without a writable record")
            assert len(server.requests) == count, "persistence failure admitted a model call"
            assert any("persist" in e.get("message", "") for e in app.events())
        finally:
            app.close()


def recovery_completed_tool(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-completed-tool-") as directory:
        server.worker_tool = {"name":"bash", "arguments":{"command":"printf x >> completed-counter; printf retained-tool-result"}}
        server.delay_tool_result = True
        app = App(directory, server)
        try:
            os.write(app.master, b"/task preserve a completed tool result\r")
            def completed():
                _, record = app.record()
                return any(o["complete"] and o.get("result", {}).get("output") == "retained-tool-result" for o in record["operations"] if o.get("result"))
            app.wait_for(completed)
            path, _ = app.record()
            app.process.kill()
            app.process.wait(timeout=3)
        finally:
            app.close()
            server.worker_tool = None
            server.delay_tool_result = False
        count = len(server.requests)
        app = App(directory, server, ["--resume", str(path)])
        try:
            app.send("continue without inspection")
            assert len(server.requests) == count
            assert any(e["type"] == "retained_tool" and "retained-tool-result" in json.dumps(e) for e in app.events())
            app.send("/reconcile inspected completed-counter and retained successful tool result")
            app.send("continue after inspection")
            assert len(server.requests) == count + 1
            assert (app.workspace / "completed-counter").read_text() == "x"
            assert "retained-tool-result" in json.dumps(server.requests[-1]), "completed result lost from restored model context"
        finally:
            app.close()


def recovery_before_execution(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-before-execution-") as directory:
        server.worker_tool = {"name":"write", "arguments":{"path":"unexecuted", "content":"must not be written"}}
        server.delay_worker = True
        app = App(directory, server)
        try:
            before = len(server.requests)
            os.write(app.master, b"/task interrupt before the provider returns a tool request\r")
            app.wait_for(lambda: len(server.requests) > before)
            path, record = app.record()
            assert any(not o["complete"] and o["call"] is None for o in record["operations"])
            app.process.kill()
            app.process.wait(timeout=3)
        finally:
            app.close()
            server.worker_tool = None
            server.delay_worker = False
        before = len(server.requests)
        app = App(directory, server, ["--resume", str(path)])
        try:
            app.send("continue without inspecting the interrupted admission")
            assert len(server.requests) == before
            assert not (app.workspace / "unexecuted").exists()
            app.send("/reconcile inspected workspace; no tool was admitted and no file was created")
            app.send("continue after inspection")
            assert len(server.requests) == before + 1
            assert not (app.workspace / "unexecuted").exists()
        finally:
            app.close()


def cancelled_model_requires_inspection(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-cancelled-model-") as directory:
        app = App(directory, server)
        server.delay_worker = True
        try:
            before = len(server.requests)
            finished = sum(e["type"] == "turn_finished" for e in app.events())
            os.write(app.master, b"/task inspect any interrupted model admission\r")
            app.wait_for(lambda: len(server.requests) > before)
            os.write(app.master, b"\x1b")
            app.wait_for(lambda: sum(e["type"] == "turn_finished" for e in app.events()) > finished, timeout=2)
            _, record = app.record()
            assert record["recovery_pending"], "cancelled model admission was automatically reconciled"
            assert any(not o["complete"] and not o["reconciled"] for o in record["operations"])
            assert record["allocation"]["usage"]["unknown_cost"]
            before = len(server.requests)
            app.send("continue before inspection")
            assert len(server.requests) == before
            app.send("/reconcile inspected the interrupted request; no tool ran and billing is unknown")
            server.delay_worker = False
            app.send("continue after inspecting the interrupted request")
            assert len(server.requests) == before + 1
            _, record = app.record()
            assert not record["recovery_pending"]
            assert record["allocation"]["model_calls"] == 2
        finally:
            server.delay_worker = False
            app.close()


def ordinary_home_reconciliation(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-home-reconcile-") as directory:
        app = App(directory, server, home_workspace=True)
        secret = "synthetic-home-file-not-for-task-capture"
        (Path(directory) / "private-canary").write_text(secret)
        server.delay_worker = True
        try:
            before = len(server.requests)
            finished = sum(e["type"] == "turn_finished" for e in app.events())
            os.write(app.master, b"ordinary conversation at the home directory\r")
            app.wait_for(lambda: len(server.requests) > before)
            os.write(app.master, b"\x1b")
            app.wait_for(lambda: sum(e["type"] == "turn_finished" for e in app.events()) > finished, timeout=2)
            path, record = app.record()
            assert record["recovery_pending"]
            app.send("/reconcile inspected interrupted conversation and home workspace")
            server.delay_worker = False
            app.send("continue the ordinary conversation")
            _, record = app.record()
            assert not record["recovery_pending"]
            assert record["last_snapshot"] is None
            assert secret not in (path / "state.json").read_text()
            decisions = record["decisions"]
        finally:
            server.delay_worker = False
            app.close()
        before = len(server.requests)
        app = App(directory, server, ["--resume", str(path)], home_workspace=True)
        try:
            app.send("continue before workspace inspection")
            assert len(server.requests) == before
            app.send("/reconcile inspected home workspace after reopening")
            app.send("continue after workspace inspection")
            assert len(server.requests) == before + 1
            _, record = app.record()
            assert record["decisions"][:-1] == decisions
            assert secret not in (path / "state.json").read_text()
        finally:
            app.close()


def unsupported_backend_recovery(server):
    previous = server.adapter
    try:
        for adapter in ("codex", "claude"):
            with tempfile.TemporaryDirectory(prefix="demoncoder-backend-capability-") as directory:
                server.adapter = adapter
                before = len(server.requests)
                app = App(directory, server)
                try:
                    app.send("/task require enforceable native allocations")
                    assert any("explicit tasks require a native" in e.get("message", "") for e in app.events())
                    path, record = app.record()
                    assert not record["operations"]
                    assert len(server.requests) == before
                finally:
                    app.close()
                app = App(directory, server, ["--resume", str(path)], expect_start=False)
                try:
                    assert b"cannot restore its internal conversation" in app.output, app.output
                    assert len(server.requests) == before
                finally:
                    app.close()
    finally:
        server.adapter = previous


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--requirement", required=True)
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), WorkflowProvider)
    server.tool_cycles = False
    server.received = []
    server.requests = []
    server.reviews = []
    server.edit_work = False
    server.verdict = {"verdict": "clear", "findings": [], "explanation": "Fixture review"}
    server.review_tool = None
    server.worker_tool = None
    server.oracles = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        if args.requirement == "VERIFY-001":
            acceptance_without_checks(server)
            failed_corrected_and_stale(server)
        elif args.requirement == "VERIFY-002":
            verification_confinement_and_cancel(server)
            verification_snapshot_failure_retains_result(server)
        elif args.requirement == "VERIFY-003":
            reviewer_negative_cases(server)
        elif args.requirement == "VERIFY-004":
            bounded_correction(server)
            interrupted_reverification(server)
            interrupted_review(server)
        elif args.requirement == "VERIFY-005":
            cumulative_allocations(server)
        elif args.requirement == "VERIFY-006":
            for adapter in ("anthropic-api", "openai-api"):
                server.adapter = adapter
                interrupted_verification_attribution(server)
                recovery_cases(server)
                recovery_refusals(server)
                recovery_completed_tool(server)
                recovery_before_execution(server)
                cancelled_model_requires_inspection(server)
                ordinary_home_reconciliation(server)
            unsupported_backend_recovery(server)
        else:
            raise AssertionError(f"production coverage not implemented for {args.requirement}")
    finally:
        server.shutdown()
        server.server_close()
    print(f"{args.requirement}: production terminal cases passed")


if __name__ == "__main__":
    main()
