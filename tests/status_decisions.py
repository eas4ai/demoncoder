#!/usr/bin/env python3
"""Current-screen checks for status and read-only evidence inspection."""
import argparse
import os
import select
import sys
import tempfile
import time
from pathlib import Path

sys.dont_write_bytecode = True
from scrollback import App as ScrollApp, row_numbers
from terminal_screen import screen_text
from advanced_orchestration import scenario, assign, heartbeat_call, heartbeat, ready
from assignable_subagents import agent

F2 = b"\x1bOQ"


def screen(app, predicate, description, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if select.select([app.master], [], [], .03)[0]:
            app.output.extend(os.read(app.master, 65536))
        value = screen_text(app.output, 180, 40)
        if predicate(value):
            return value
        assert app.process.poll() is None, "app stopped before " + description
    raise AssertionError(description + "\n" + value)


def counts():
    with scenario() as (app, server, project):
        first = assign(app, server, "anthropic-api", calls=[heartbeat_call("count-heartbeat")],
                       owned=("count-heartbeat",))
        heartbeat(app, first, "count-heartbeat")
        second = assign(app, server, "openai-api", dependencies=(first,))
        app.wait_for(lambda: agent(app, second)["status"] == "queued")
        screen(app, lambda s: "agents 1 active" in s and "1 waiting" in s,
               "actual active and waiting counts")
        app.send(f"/agent-cancel {first}")
        screen(app, lambda s: "agents 0 active" in s and "1 waiting" in s and "1 held" in s,
               "counts after individual cancellation")


def evidence():
    with scenario() as (app, server, project):
        identifier = assign(app, server, "anthropic-api", calls=[{"name":"write", "arguments":{"path":"greeting", "content":"begin\n" + "evidence-λ\n" * 1600 + "end-marker\n"}}])
        retained = ready(app, identifier)
        screen(app, lambda s: "1 ready" in s, "ready state reaches terminal")
        record_path, _ = app.record()
        before = (record_path / "state.json").read_bytes()
        original = (project / "greeting").read_bytes()
        os.write(app.master, F2 + b"\t")
        screen(app, lambda s: "Inspection" in s and "Objective:" in s
               and "child-anthropic-api" in s and "Freshness:" in s,
               "readable agent evidence and freshness")
        os.write(app.master, b"\x1b[6~")
        screen(app, lambda s: "Checks" in s or "evidence-λ" in s, "inspect check/source sections")
        os.write(app.master, b"\x1b[C")
        screen(app, lambda s: "Page 2" in s and "evidence-λ" in s, "next bounded evidence page")
        os.write(app.master, b"\x1b[D")
        screen(app, lambda s: "Page 1" in s and "Objective:" in s, "previous evidence page")
        os.write(app.master, F2)
        screen(app, lambda s: "Inspection ·" not in s, "close inspection")
        assert (record_path / "state.json").read_bytes() == before, "inspection mutated durable evidence"
        assert (project / "greeting").read_bytes() == original, "navigation integrated child changes"
        app.send(f"/agent {identifier}")
        texts = [e["text"] for e in app.events() if e["type"] == "text"]
        assert any("Objective:" in text and "Freshness:" in text for text in texts), texts[-3:]
        assert agent(app, identifier)["checks"] == retained["checks"], "inspection rewrote original checks"


def actions():
    with scenario() as (app, server, project):
        first = assign(app, server, "anthropic-api")
        ready(app, first)
        second = assign(app, server, "openai-api", dependencies=(first,))
        app.wait_for(lambda: agent(app, second)["status"] == "queued")
        screen(app, lambda s: "1 waiting" in s and "1 ready" in s, "dependency state reaches terminal")
        os.write(app.master, F2 + b"\t")
        screen(app, lambda s: "/agent-integrate 1" in s and "recheck" in s,
               "explicit integration consequence")
        os.write(app.master, b"\t")
        screen(app, lambda s: "Prerequisites:" in s and "Waiting" in s,
               "dependency reason")
        assert not (project / "greeting").read_text().startswith("updated"), "navigation had effects"
        assert agent(app, first)["status"] == "ready", "navigation integrated ready child"
        assert agent(app, second)["status"] == "queued", "navigation released dependent"


def navigation():
    with tempfile.TemporaryDirectory(prefix="demoncoder-inspection-") as directory:
        app = ScrollApp(Path(directory))
        try:
            app.send(b"\x0fscroll-check\r")
            app.wait(lambda s: "ROW-00399" in s, "streaming tail")
            app.send(b"\x1b[5~")
            old = app.wait(lambda s: bool(row_numbers(s)) and max(row_numbers(s)) < 399,
                           "conversation history")
            anchor = min(row_numbers(old))
            app.send("draft-λ".encode() + F2)
            app.wait(lambda s: "Inspection" in s and "draft-λ" in s, "open inspection with draft")
            app.send(b"\t\x1b[6~")
            app.resize(4, 8)
            app.resize(35, 100)
            app.send(F2)
            app.wait(lambda s: "draft-λ" in s and bool(row_numbers(s))
                     and min(row_numbers(s)) == anchor, "restore draft and history")
            app.send(F2 + b"\x03")
            app.wait(lambda s: "cancelled" in s.splitlines()[0], "cancel while inspecting", timeout=2)
        finally:
            app.close()




def task_evidence():
    with scenario() as (app, server, project):
        app.send("/task inspect parent evidence")
        original = (project / "greeting").read_bytes()
        (project / "greeting").write_bytes(b"")
        app.send("/verify")
        assert app.state()["verification"] == "failed"
        (project / "greeting").write_bytes(original)
        app.send("/verify")
        assert app.state()["verification"] == "passed"
        app.send("/task-status")
        texts = [event["text"] for event in app.events() if event["type"] == "text"]
        inspection = next(text for text in reversed(texts) if "Inspection · Task" in text)
        for value in ("inspect parent evidence", "recorded fail", "recorded pass", "files not rechecked", "executed-check-stdout", "executed-check-stderr"):
            assert value in inspection, (value, inspection)
        screen(app, lambda s: "Task 1" in s and "Accepted no" in s, "separate task acceptance state")
        os.write(app.master, F2 + b"\t")
        screen(app, lambda s: "Inspection · Task 1" in s and "Freshness:" in s, "task inspection")
        # Changing files while the view is open must never create a current approval.
        (project / "greeting").write_text("changed after checks\n")
        os.write(app.master, b"\x1b[15~")  # F5 refreshes saved state only.
        screen(app, lambda s: "files not rechecked" in s, "explicit freshness limit after file edit")
        os.write(app.master, F2)
        app.send("/accept")
        assert app.record()[1]["task"]["accepted"] is None, "unchecked files were accepted"
        errors = [event["message"] for event in app.events() if event["type"] == "error"]
        assert any("current workspace" in error for error in errors), errors


def recovery_inspection():
    from advanced_orchestration import launch
    with scenario() as (app, server, project):
        first = assign(app, server, "anthropic-api", calls=[heartbeat_call("resume-heartbeat")],
                       owned=("resume-heartbeat",))
        heartbeat(app, first, "resume-heartbeat")
        record_path, _ = app.record()
        app.process.kill()
        app.process.wait(timeout=5)
        os.close(app.master)
        app.master = None
        resumed = launch(str(project.parent), server, ("--resume", str(record_path)))
        try:
            screen(resumed, lambda s: "Inspection required" in s and "1 held" in s,
                   "restored uncertainty in current status")
            before = (record_path / "state.json").read_bytes()
            requests = len(server.requests)
            os.write(resumed.master, F2 + b"\t")
            screen(resumed, lambda s: "Uncertain" in s and "/agent-reconcile 1" in s,
                   "inspect interrupted work without replay")
            os.write(resumed.master, b"\x1b[C\x1b[D")
            assert len(server.requests) == requests, "inspection replayed a provider request"
            assert (record_path / "state.json").read_bytes() == before, "inspection changed recovery evidence"
        finally:
            resumed.close()

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--requirement", choices=["REM-001", "REM-002", "REM-003", "REM-004"], required=True)
    args = parser.parse_args()
    {"REM-001": counts, "REM-002": evidence, "REM-003": actions, "REM-004": navigation}[args.requirement]()
    if args.requirement == "REM-002":
        task_evidence()
    if args.requirement == "REM-004":
        recovery_inspection()
    print(args.requirement + ": production inspection passed", flush=True)
