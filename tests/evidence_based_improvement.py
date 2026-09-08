#!/usr/bin/env python3
"""Learning proof through production PTYs, executed checks and actual adapters."""
import argparse
import ctypes
from contextlib import contextmanager
import http.server
import json
import os
import signal
import select
import struct
import subprocess
import time
from pathlib import Path
import sys
import tempfile
import threading

sys.dont_write_bytecode = True
from verification_workflow import App, WorkflowProvider

CHECK = "test -s greeting"
FLAGS = ["--check", CHECK, "--reviewer", "worker"]
CLAIM = "GREETING-LESSON: verify nonempty greeting output before accepting a change."


@contextmanager
def provider():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), WorkflowProvider)
    server.tool_cycles = False
    server.received, server.requests, server.reviews, server.oracles = [], [], [], []
    server.edit_work = False
    server.verdict = {"verdict": "clear", "findings": [], "explanation": "The controlled reviewer checked the supplied source change and executed assertion."}
    server.review_tool = server.worker_tool = None
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        yield server
    finally:
        server.shutdown()
        server.server_close()


def catalog(app):
    records = list((app.root / ".demoncoder/learning").glob("*/state.json"))
    assert len(records) == 1, records
    envelope = json.loads(records[0].read_text())
    return records[0], envelope["payload"]


def command(app, text, *, recovery_notice=False):
    before = len(app.events())
    if len(text.encode()) > 1024:
        finished = sum(e["type"] == "turn_finished" for e in app.events())
        os.write(app.master, b"\x1b[200~" + text.encode() + b"\x1b[201~\r")
        app.wait_for(lambda: sum(e["type"] == "turn_finished" for e in app.events()) > finished)
    else:
        app.send(text)
    events = app.events()[before:]
    errors = [e["message"] for e in events if e["type"] == "error"]
    if recovery_notice:
        errors = [error for error in errors if not error.startswith("An interrupted operation may have partial effects.")]
    assert not errors, (text, errors)
    return events


def refused(app, text, fragment):
    before = len(app.events())
    app.send(text)
    errors = [e["message"] for e in app.events()[before:] if e["type"] == "error"]
    assert any(fragment in error for error in errors), (text, fragment, errors)


def discovery(app, server):
    (app.workspace / "greeting").write_text("")
    command(app, "/task diagnose empty greeting")
    command(app, "/verify")
    record_path, record = app.record()
    original = record["task"]["checks"][0].copy()
    assert original["success"] is False and original["exit_code"] != 0
    requests = len(server.requests)
    operations = len(record["operations"])
    command(app, "/improvements")
    _, retained = catalog(app)
    assert len(retained["observations"]) == len(retained["candidates"]) == 1
    observation = retained["observations"][0]
    assert observation["source"]["session"] == record_path.name
    assert observation["snapshot"] == original["snapshot"]
    assert observation["source"]["receipt"] == {"kind": "task_check", "task": 1, "round": 0, "index": 0}
    command(app, "/improvements")
    command(app, "/improvement-note 1 Developer observed empty output; cause remains unproven.")
    events = command(app, "/improvement 1")
    text = "\n".join(e.get("text", "") for e in events)
    for required in ("Original cited receipt", "behavioral_check", "scope", "benefit", "risks", "proposed", "saved evidence"):
        assert required in text, (required, text)
    _, retained = catalog(app)
    assert len(retained["observations"]) == 1
    assert retained["observations"][0]["annotations"][0]["author"] == "developer"
    assert app.record()[1]["task"]["checks"][0] == original
    assert len(server.requests) == requests, "browsing or annotations invoked a provider"
    assert len(app.record()[1]["operations"]) == operations, "browsing admitted a tool"
    assert (app.workspace / "greeting").read_text() == ""
    return original


def supported(app, server):
    discovery(app, server)
    command(app, "/abandon")
    requests = len(server.requests)
    refused(app, "/lesson-propose 1 " + json.dumps({"claim": CLAIM, "keywords": ["greeting"]}), "supported outcome")
    assert len(server.requests) == requests
    command(app, "/improve 1")
    task = app.record()[1]["task"]
    assert task["improvement"]["candidate"] == 1
    assert task["commands"] == [CHECK]
    assert any("Developer-authorized improvement candidate 1" in prompt and "Original source evidence" in prompt
               and '"success":false' in prompt for prompt in server.received), server.received
    command(app, "/verify")
    assert catalog(app)[1]["candidates"][0]["outcomes"][-1]["status"] == "unresolved"
    refused(app, "/accept", "selected checks")
    # The controlled provider executes the correction through the production write tool.
    server.worker_tool = {"name": "write", "arguments": {"path": "greeting", "content": "corrected greeting\n"}}
    command(app, "/correct")
    server.worker_tool = None
    assert (app.workspace / "greeting").read_text() == "corrected greeting\n"
    outcomes = catalog(app)[1]["candidates"][0]["outcomes"]
    assert outcomes[0]["status"] == "unresolved" and outcomes[-1]["status"] == "supported", outcomes
    command(app, "/accept")
    assert app.state()["accepted"]
    command(app, "/lesson-propose 1 " + json.dumps({"claim": CLAIM, "keywords": ["greeting"]}))
    assert not catalog(app)[1]["lessons"][0]["enabled"]
    command(app, "/lesson-enable 1")
    assert catalog(app)[1]["lessons"][0]["enabled"]
    return app.record()[0]


def observations_and_candidates(requirement):
    with provider() as server, tempfile.TemporaryDirectory(prefix="demoncoder-learning-") as directory:
        app = App(directory, server, FLAGS)
        try:
            discovery(app, server)
            before = len(server.requests)
            refused(app, "/improvement-propose 999 {}", "proposal JSON")
            proposal = {"objective": "Repair greeting", "scope": "greeting only", "benefit": "Proposed empty-output repair",
                        "behavioral_check": CHECK, "risks": "Cause is a hypothesis"}
            refused(app, "/improvement-propose 999 " + json.dumps(proposal), "observation not retained")
            command(app, "/improvement-propose 1 " + json.dumps(proposal))
            assert len(catalog(app)[1]["candidates"]) == 2
            assert len(server.requests) == before
            refused(app, "/improvement-note task-check:999:0:0 nonexistent evidence", "source task is missing")
            assert len(catalog(app)[1]["observations"]) == 1
            # A real passing task receipt can also carry an attributed annotation.
            (app.workspace / "greeting").write_text("nonempty")
            command(app, "/verify")
            command(app, "/review")
            command(app, "/improvement-note task-review:1 Developer checked the retained review.")
            assert len(catalog(app)[1]["observations"]) == 2
            events = command(app, "/observation 2")
            assert any("Developer checked the retained review" in event.get("text", "") for event in events)
            path, _ = app.record()
            app.close()
            app = App(directory, server, [*FLAGS, "--resume", str(path)])
            command(app, "/improvements")
            assert len(catalog(app)[1]["observations"]) == 2, "restart duplicated the failed check"
            if requirement == "LEARN-001":
                source = path / "state.json"
                original = source.read_bytes()
                # A separate session can safely inspect a missing source without
                # damaging its own current record.
                app.close()
                source.rename(path / "saved-state.json")
                app = App(directory, server, FLAGS)
                refused(app, "/improvement-propose 1 " + json.dumps(proposal), "source unavailable")
                (path / "saved-state.json").rename(source)
                assert source.read_bytes() == original
        finally:
            app.close()


def corrections_and_lessons(requirement):
    for adapter in ("anthropic-api", "openai-api"):
        with provider() as server, tempfile.TemporaryDirectory(prefix="demoncoder-learning-") as directory:
            server.adapter = adapter
            app = App(directory, server, FLAGS)
            try:
                supported(app, server)
                requests = len(server.requests)
                refused(app, "/improve 1", "authorization already recorded")
                assert len(server.requests) == requests
                if requirement == "LEARN-005":
                    command(app, "/lesson-propose 1 " + json.dumps({"claim": "Replacement greeting guidance", "keywords": ["greeting"]}))
                    refused(app, "/lesson-supersede 1 2", "enable")
                    command(app, "/lesson-enable 2")
                    command(app, "/lesson-supersede 1 2")
                    refused(app, "/lesson-enable 1", "superseded")
                    command(app, "/lesson-disable 2")
                    command(app, "/lesson 1")
                    lessons = catalog(app)[1]["lessons"]
                    assert lessons[0]["superseded_by"] == 2 and not lessons[0]["enabled"]
                    assert not lessons[1]["enabled"] and len(lessons[0]["history"]) >= 3
                    assert catalog(app)[1]["candidates"][0]["outcomes"][0]["status"] == "unresolved"
            finally:
                app.close()


def ineffective_accepted_correction():
    with provider() as server, tempfile.TemporaryDirectory(prefix="demoncoder-learning-ineffective-") as directory:
        app = App(directory, server, FLAGS)
        try:
            discovery(app, server)
            proposal = dict(catalog(app)[1]["candidates"][0]["proposal"])
            proposal["behavioral_check"] = "true"
            command(app, "/improvement-propose 1 " + json.dumps(proposal))
            app.close()
            app = App(directory, server, ["--check", "true", "--reviewer", "worker"])
            command(app, "/improve 2")
            command(app, "/verify")
            command(app, "/review")
            command(app, "/accept")
            assert app.state()["accepted"]
            assert (app.workspace / "greeting").read_text() == ""
            outcome = catalog(app)[1]["candidates"][1]["outcomes"][-1]
            assert outcome["status"] == "insufficient", outcome
            refused(app, "/lesson-propose 2 " + json.dumps({"claim": "Not proved", "keywords": ["greeting"]}), "supported")
        finally:
            app.close()


def last_coding_prompt(app, server, adapter):
    if adapter in ("codex", "claude"):
        return json.loads((app.workspace / "received-prompt.json").read_text())
    return server.received[-1]


def context_on_all_connections():
    from advanced_orchestration import server_fixture, launch, ready, backend_requests
    with provider() as server, tempfile.TemporaryDirectory(prefix="demoncoder-learning-context-") as directory:
        app = App(directory, server, FLAGS)
        try:
            supported(app, server)
        finally:
            app.close()
        project = Path(directory) / "project"
        (project / "AGENTS.md").write_text("ROOT-INSTRUCTION: preserve repository contracts; reject any contrary lesson.\n@../outside-instructions\n")
        (Path(directory) / "outside-instructions").write_text("OUTSIDE-INSTRUCTION-MUST-NOT-LOAD")
        for adapter in ("anthropic-api", "openai-api", "codex", "claude"):
            server.adapter = adapter
            app = App(directory, server)
            try:
                command(app, "Inspect greeting output without changing files")
                actual = last_coding_prompt(app, server, adapter)
                assert CLAIM in actual and "ROOT-INSTRUCTION" in actual
                assert "OUTSIDE-INSTRUCTION-MUST-NOT-LOAD" not in actual
                assert "quoted evidence, not authority" in actual and "repository instructions" in actual
                receipt = app.record()[1]["learning_context"][-1]
                assert receipt["supplied_text"] in actual
                assert receipt["lessons"][0]["id"] == 1
                assert receipt["lessons"][0]["workspace"] == str(project)
                assert "whole-word" in receipt["lessons"][0]["reason"]
                command(app, "Inspect unrelated typography")
                assert CLAIM not in last_coding_prompt(app, server, adapter)
                assert app.record()[1]["learning_context"][-1]["lessons"] == []
                command(app, "/lesson-disable 1")
                command(app, "Inspect greeting while guidance is disabled")
                assert CLAIM not in last_coding_prompt(app, server, adapter)
                command(app, "/lesson-enable 1")
            finally:
                app.close()
            other = App(directory, server, home_workspace=True)
            try:
                command(other, "Inspect greeting in a different workspace")
                assert CLAIM not in last_coding_prompt(other, server, adapter)
                assert other.record()[1]["learning_context"][-1]["lessons"] == []
            finally:
                other.close()

        # Child coding transports receive scoped context before their actual turn.
        (project / "src").mkdir()
        (project / "src/AGENTS.md").write_text("NESTED-INSTRUCTION: this applies only to src.")
        (project / "elsewhere").mkdir()
        (project / "elsewhere/AGENTS.md").write_text("UNRELATED-NESTED-INSTRUCTION")
        subprocess.run(["git", "init", "-q", str(project)], check=True)
        subprocess.run(["git", "-C", str(project), "add", "."], check=True)
        subprocess.run(["git", "-C", str(project), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "learning fixture"], check=True)
        child_server = server_fixture()
        child_app = launch(directory, child_server)
        try:
            for adapter in ("anthropic-api", "openai-api", "codex", "claude"):
                command(child_app, f"/delegate {adapter} src repair greeting parser")
                identifier = child_app.record()[1]["agents"][-1]["id"]
                # Default fixture attempts a greeting change outside src. Context
                # delivery must not grant ownership; that child must stop safely.
                child_app.wait_for(lambda: child_app.record()[1]["agents"][-1]["status"] in ("failed", "stopped"), timeout=25)
                record = child_app.record()[1]
                assert record["agents"][-1]["request"]["objective"] == "repair greeting parser", "prepared context changed developer control arguments"
                receipt = next(r for r in reversed(record["learning_context"]) if r["target"] == f"agent:{identifier}")
                if adapter in ("codex", "claude"):
                    actual = [r["prompt"] for r in backend_requests(directory) if r["kind"] == "prompt" and r["adapter"] == adapter][-1]
                else:
                    requests = [r for r in child_server.requests if r["model"] == "child-" + adapter and r.get("tools")]
                    history = requests[-1].get("messages", requests[-1].get("input"))
                    actual = next(m["content"] for m in reversed(history) if isinstance(m.get("content"), str) and m["content"].startswith("You are assigned child agent"))
                assert receipt["supplied_text"] in actual and CLAIM in actual
                assert "NESTED-INSTRUCTION" in actual and "UNRELATED-NESTED-INSTRUCTION" not in actual
                assert {i["scope"] for i in receipt["instructions"]} == {".", "src"}
                assert (project / "greeting").read_text() == "corrected greeting\n", "context selection granted integration"
                command(child_app, f"/improvement-note agent-check:{identifier}:1:0 Annotate retained child check.") if record["agents"][-1]["checks"] else None
                command(child_app, f"/delegate {adapter} greeting repair greeting output")
                ready_id = child_app.record()[1]["agents"][-1]["id"]
                child = ready(child_app, ready_id)
                assert child["status"] == "ready" and child["integration"] is None
                assert (project / "greeting").read_text() == "corrected greeting\n", "ready lesson-guided child was integrated without a developer command"
                command(child_app, f"/improvement-note agent-review:{ready_id} Developer annotation of retained child review.")
        finally:
            child_app.close()
            child_server.shutdown()
            child_server.server_close()


def recovery_boundaries():
    with provider() as server, tempfile.TemporaryDirectory(prefix="demoncoder-learning-recovery-") as directory:
        app = App(directory, server, FLAGS)
        try:
            discovery(app, server)
            command(app, "/abandon")
            source_path, _ = app.record()
            catalog_path, _ = catalog(app)
            # Observe atomic publication, then stop the production process at its
            # durable authorization boundary. No runtime testing hook is involved.
            libc = ctypes.CDLL(None, use_errno=True)
            descriptor = libc.inotify_init1(os.O_CLOEXEC | os.O_NONBLOCK)
            assert descriptor >= 0
            assert libc.inotify_add_watch(descriptor, os.fsencode(catalog_path.parent), 0x80) >= 0  # IN_MOVED_TO
            requests = len(server.requests)
            try:
                os.write(app.master, b"/improve 1\r")
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline:
                    if select.select([descriptor], [], [], .01)[0]:
                        os.read(descriptor, 65536)
                        if catalog(app)[1]["candidates"][0]["authorization"]:
                            os.kill(app.process.pid, signal.SIGSTOP)
                            break
                else:
                    raise AssertionError("authorization was never published")
                reservation = catalog(app)[1]["candidates"][0]["authorization"].copy()
                assert reservation["session"] == source_path.name and reservation["task"] == 2
                app.process.kill()
                app.process.wait(timeout=3)
            finally:
                os.close(descriptor)
            app.close()
            before = len(server.requests)
            app = App(directory, server, [*FLAGS, "--resume", str(source_path)])
            command(app, "/improvement 1", recovery_notice=True)
            assert len(server.requests) == before, "restart replayed authorized work"
            if app.record()[1]["recovery_pending"]:
                command(app, "/reconcile inspected stopped authorization; no task effect requires replay")
            if app.record()[1]["task"] is not None:
                command(app, "/abandon")
            refused(app, "/improve 1", "authorization already recorded")
            assert len(server.requests) == before
            assert catalog(app)[1]["candidates"][0]["authorization"] == reservation
            assert (app.workspace / "greeting").read_text() == ""

            # A distinct developer proposal may authorize genuinely new work.
            proposal = catalog(app)[1]["candidates"][0]["proposal"]
            command(app, "/improvement-propose 1 " + json.dumps(proposal))
            server.delay_worker = True
            os.write(app.master, b"/improve 2\r")
            app.wait_for(lambda: len(server.requests) > before)
            source_path, consumed = app.record()
            app.process.kill()
            app.process.wait(timeout=3)
            app.close()
            count = len(server.requests)
            server.delay_worker = False
            app = App(directory, server, [*FLAGS, "--resume", str(source_path)])
            command(app, "/improvement 2", recovery_notice=True)
            record = app.record()[1]
            assert record["recovery_pending"]
            assert record["task"]["improvement"]["candidate"] == 2
            assert record["allocation"]["model_calls"] == consumed["allocation"]["model_calls"]
            assert record["allocation"]["tool_calls"] == consumed["allocation"]["tool_calls"]
            assert len(server.requests) == count
        finally:
            app.close()


def bounded_inspection():
    import fcntl
    import termios
    from status_decisions import screen, F2
    with provider() as server, tempfile.TemporaryDirectory(prefix="demoncoder-learning-limits-") as directory:
        app = App(directory, server, FLAGS)
        try:
            discovery(app, server)
            for _ in range(5):
                command(app, "/improvement-note 1 " + "λ" * 1800)
            events = command(app, "/improvement 1 2")
            text = next(e["text"] for e in events if e["type"] == "text" and "Inspection" in e["text"])
            assert "λ" in text and len(text.encode()) < 9000 and "Page 2" in text
            path, before = catalog(app)
            descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
                refused(app, "/improvements", "busy")
                assert catalog(app)[1] == before
            finally:
                os.close(descriptor)
            os.write(app.master, "draft-λ".encode() + F2)
            screen(app, lambda s: "Inspection" in s and "draft-λ" in s, "learning inspection preserves input")
            fcntl.ioctl(app.master, termios.TIOCSWINSZ, struct.pack("HHHH", 4, 8, 0, 0))
            fcntl.ioctl(app.master, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
            os.write(app.master, F2 + b"\x7f" * len("draft-λ"))
            proposal = before["candidates"][0]["proposal"]
            for _ in range(63):
                command(app, "/improvement-propose 1 " + json.dumps(proposal))
            assert len(catalog(app)[1]["candidates"]) == 64
            requests = len(server.requests)
            refused(app, "/improvement-propose 1 " + json.dumps(proposal), "retention is full")
            assert len(catalog(app)[1]["candidates"]) == 64 and len(server.requests) == requests
            command(app, "/improvement 1")
            assert "files not rechecked" in "\n".join(e.get("text", "") for e in app.events())
            command(app, "/abandon")
            server.delay_worker = True
            requests = len(server.requests)
            finished = sum(e["type"] == "turn_finished" for e in app.events())
            os.write(app.master, b"/improve 1\r")
            app.wait_for(lambda: len(server.requests) > requests)
            os.write(app.master, F2 + b"\x03")
            app.wait_for(lambda: sum(e["type"] == "turn_finished" for e in app.events()) > finished, timeout=2)
            assert catalog(app)[1]["candidates"][0]["authorization"] is not None
            assert app.record()[1]["recovery_pending"]
            assert (app.workspace / "greeting").read_text() == ""
        finally:
            app.close()

    with provider() as server, tempfile.TemporaryDirectory(prefix="demoncoder-learning-selection-") as directory:
        app = App(directory, server, FLAGS)
        try:
            supported(app, server)
            for identifier in range(2, 6):
                command(app, "/lesson-propose 1 " + json.dumps({"claim": f"Additional greeting guidance {identifier}", "keywords": ["greeting"]}))
                command(app, f"/lesson-enable {identifier}")
            command(app, "/task inspect greeting with bounded applicable context")
            receipt = app.record()[1]["learning_context"][-1]
            assert len(receipt["lessons"]) == 4 and receipt["omitted_matches"] == 1
            assert receipt["supplied_text"] in server.received[-1] and "omitted 1" in server.received[-1]
            events = command(app, "/learning-context")
            assert any("Prepared coding context" in event.get("text", "") for event in events)
        finally:
            app.close()



def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--requirement", required=True, choices=[f"LEARN-{n:03}" for n in range(1, 9)])
    requirement = parser.parse_args().requirement
    if requirement in ("LEARN-001", "LEARN-002"):
        observations_and_candidates(requirement)
    elif requirement in ("LEARN-003", "LEARN-004", "LEARN-005"):
        corrections_and_lessons(requirement)
        if requirement == "LEARN-004":
            ineffective_accepted_correction()
    elif requirement == "LEARN-006":
        context_on_all_connections()
    elif requirement == "LEARN-007":
        recovery_boundaries()
    elif requirement == "LEARN-008":
        bounded_inspection()
    print(requirement + ": production evidence-based improvement passed", flush=True)


if __name__ == "__main__":
    main()
