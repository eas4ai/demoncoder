#!/usr/bin/env python3
"""Real role requests, queued identity and recovery across live Settings saves."""
import http.server
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
import sys

sys.dont_write_bytecode = True
from verification_workflow import App
from onboarding import App as ScreenApp
from terminal_session import Provider
from assignable_subagents import repository, agent
from orchestration_backend_fixture import role_request
from tool_cycle_fixture import sse_call
from live_settings import open_settings, check_providers, choose_role, save_settings

MODELS = ["fixture-model", "creator-new", "worker-old", "worker-new", "oracle-model",
          "reviewer-old", "reviewer-new", "advisor-model", "judge-model"]


class RoleProvider(Provider):
    def do_GET(self):
        if self.path.startswith("/models/"):
            return super().do_GET()
        assert self.path == "/models", self.path
        assert self.headers.get("x-api-key") == "synthetic-anthropic-key"
        body = json.dumps({"data": [{"id": model} for model in MODELS]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        history = body["messages"]
        strings = [item["content"] for item in history if isinstance(item.get("content"), str)]
        prompt = strings[-1]
        supervision = role_request(prompt)
        if supervision:
            role = supervision[0]
        elif not body.get("tools"):
            role = "oracle" if "outside-access Oracle" in prompt else "reviewer"
        elif any(text.startswith("You are assigned child agent") for text in strings):
            role = "worker"
        else:
            role = "creator"
        request = {"role": role, "body": body, "complete": False}
        self.server.requests.append(request)
        if self.server.hold == role:
            self.server.hold = None
            self.server.started.set()
            assert self.server.release.wait(30), "fixture request was never released"
        if role == "oracle":
            text = json.dumps({"decision": "allow", "reason": "Disposable fixture request"})
        elif role in ("reviewer", "advisor", "worker_response", "judge"):
            assert not body.get("tools"), "decision role received tools"
            findings = ["Check the claimed greeting."] if role == "advisor" else []
            text = json.dumps({"verdict": "findings" if findings else "clear", "findings": findings,
                               "explanation": "Examined the retained runtime evidence."})
        else:
            text = f"Completed {role} using {body['model']}."
        events = [
            {"type": "message_start", "message": {"usage": {"input_tokens": 11}}},
            {"type": "content_block_delta", "delta": {"type": "text_delta", "text": text}},
            {"type": "message_delta", "usage": {"output_tokens": 8}},
            {"type": "message_stop"},
        ]
        if role == "creator" and history[-1].get("content") == "exercise assigned Oracle":
            events = sse_call(self.path, {"id": "oracle-fixture-write", "name": "write",
                              "arguments": {"path": str(self.server.oracle_path), "content": "checked fixture"}})
        data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        try:
            self.wfile.write(data)
            request["complete"] = True
        except (BrokenPipeError, ConnectionResetError):
            pass


class SettingsUI:
    """Expose raw keystrokes without changing the workflow harness command API."""
    def __init__(self, app):
        self.app = app

    def __getattr__(self, name):
        return getattr(self.app, name)

    def send(self, keys):
        os.write(self.app.master, keys.encode() if isinstance(keys, str) else keys)

    wait_current = ScreenApp.wait_current


def change(app, **roles):
    ui = SettingsUI(app)
    open_settings(ui)
    check_providers(ui, ["worker"])
    for role, model in roles.items():
        choose_role(ui, role, model, "worker", inherit=model is None)
    save_settings(ui)


def config(**roles):
    text = '\n[settings]\nproviders=["worker"]\n[settings.creator]\nconnection="worker"\nmodel="fixture-model"\n'
    for role, model in roles.items():
        text += f'[settings.overrides.{role}]\nconnection="worker"\nmodel={json.dumps(model)}\n'
    return text


class RoleSettings(unittest.TestCase):
    def setUp(self):
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), RoleProvider)
        self.server.requests = []
        self.server.extra_config = config(worker="worker-old", reviewer="reviewer-old",
                                          oracle="oracle-model", advisor="advisor-model", judge="judge-model")
        self.server.hold = None
        self.server.started = threading.Event()
        self.server.release = threading.Event()
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.directory = tempfile.TemporaryDirectory(prefix="demoncoder-role-settings-")
        repository(self.directory.name)
        self.app = None

    def tearDown(self):
        self.server.release.set()
        if self.app:
            self.app.close()
        self.server.shutdown()
        self.server.server_close()
        self.directory.cleanup()

    def launch(self, *flags, expect_start=True):
        self.app = App(self.directory.name, self.server, flags, expect_start=expect_start)
        return self.app

    def requests(self, role):
        return [r["body"] for r in self.server.requests if r["role"] == role]

    def wait_agent(self, identifier, status="ready"):
        self.app.wait_for(lambda: agent(self.app, identifier)["status"] == status, timeout=25)
        return agent(self.app, identifier)

    def test_queued_worker_captures_old_model_and_new_assignment_uses_new_default(self):
        app = self.launch("--agent-connection", "default", "--agent-limit", "1", "--check", "true",
                          "--orchestrate", "--reviewer", "default", "--judge", "default")
        self.server.hold = "worker"
        app.send("/delegate default greeting first held worker")
        app.wait_for(self.server.started.is_set)
        app.send("/delegate default greeting queued worker")
        self.assertEqual(len(app.record()[1]["agents"]), 2, app.events())
        queued = agent(app, 2)
        self.assertEqual(queued["status"], "queued")
        self.assertEqual(queued["identity"]["model"], "worker-old")
        before = app.record()[1]
        change(app, Worker="worker-new")
        self.assertEqual(len(self.requests("worker")), 1)
        self.assertEqual(agent(app, 2)["identity"], queued["identity"])
        self.assertEqual(app.record()[1]["allocation"], before["allocation"])
        self.server.release.set()
        self.wait_agent(1)
        self.wait_agent(2)
        app.send("/delegate default greeting new worker")
        self.wait_agent(3)
        self.assertEqual([r["model"] for r in self.requests("worker")], ["worker-old", "worker-old", "worker-new"])
        self.assertEqual(agent(app, 2)["identity"], queued["identity"])
        self.assertEqual((app.workspace / "greeting").read_text(), "developer dirty edit\n")

    def test_active_task_is_pinned_and_reviewer_resolves_after_correction(self):
        app = self.launch("--check", "true", "--reviewer", "default")
        app.send("/task preserve original Creator")
        original_task = app.record()[1]["task"]
        self.server.hold = "creator"
        count = sum(e["type"] == "turn_finished" for e in app.events())
        os.write(app.master, b"/correct\r")
        app.wait_for(self.server.started.is_set)
        change(app, Creator="creator-new", Reviewer="reviewer-new")
        self.assertFalse(self.requests("reviewer"))
        self.server.release.set()
        app.wait_for(lambda: sum(e["type"] == "turn_finished" for e in app.events()) > count)
        self.assertEqual([r["model"] for r in self.requests("creator")], ["fixture-model", "fixture-model"])
        self.assertEqual([r["model"] for r in self.requests("reviewer")], ["reviewer-new"])
        self.assertEqual(app.record()[1]["task"]["creator_identity"], original_task["creator_identity"])
        app.send("/abandon")
        app.send("new independent turn")
        self.assertEqual(self.requests("creator")[-1]["model"], "creator-new")

    def test_role_overrides_and_inheritance_preserve_decision_authority(self):
        app = self.launch("--agent-connection", "default", "--check", "true", "--reviewer", "default",
                          "--orchestrate", "--judge", "default")
        change(app, Creator="creator-new", Worker=None)
        app.send("/delegate default greeting inspect inherited model")
        record = self.wait_agent(1)
        app.send("/agent-cancel 1")
        app.send("/task invoke the distinct patch reviewer")
        app.send("/verify")
        app.send("/review")
        expected = {"worker": "creator-new", "reviewer": "reviewer-old", "advisor": "advisor-model",
                    "worker_response": "creator-new", "judge": "judge-model"}
        for role, model in expected.items():
            self.assertTrue(self.requests(role), (role, [e for e in app.events() if e["type"] == "error"]))
            self.assertEqual({r["model"] for r in self.requests(role)}, {model}, role)
            if role != "worker":
                self.assertTrue(all(not r.get("tools") for r in self.requests(role)), role)
        self.assertTrue(record["orchestration"]["receipts"])
        self.assertEqual((app.workspace / "greeting").read_text(), "developer dirty edit\n")

    def test_oracle_default_uses_its_assignment_without_tools(self):
        self.server.oracle_path = Path(self.directory.name) / "outside-fixture"
        app = self.launch("--yolo")
        app.send("exercise assigned Oracle")
        self.assertEqual(self.server.oracle_path.read_text(), "checked fixture")
        self.assertEqual([r["model"] for r in self.requests("oracle")], ["oracle-model"])
        self.assertTrue(all(not r.get("tools") for r in self.requests("oracle")))

    def test_explicit_child_connection_overrides_settings_worker(self):
        app = self.launch("--agent-connection", "worker", "--check", "true")
        change(app, Worker="worker-new")
        app.send("/delegate worker greeting keep the explicit connection")
        record = self.wait_agent(1, "stopped")
        self.assertEqual([r["model"] for r in self.requests("worker")], ["fixture-model"])
        self.assertEqual(record["identity"]["model"], "fixture-model")
        self.assertEqual(record["request"]["connection"], "worker")

    def test_recovery_keeps_queued_identity_allocation_and_new_reviewer_default(self):
        flags = ("--agent-connection", "default", "--agent-limit", "1", "--check", "true", "--reviewer", "default",
                 "--orchestrate", "--judge", "default")
        app = self.launch(*flags)
        app.send("/task retain task recovery")
        self.server.hold = "worker"
        app.send("/delegate default greeting interrupted worker")
        app.wait_for(self.server.started.is_set)
        app.send("/delegate default greeting retained queued worker")
        change(app, Worker="worker-new", Reviewer="reviewer-new")
        path, saved = app.record()
        # The fixture constructor recreates config, so retain the exact Settings save.
        persisted = app.config.read_text()
        self.server.extra_config = persisted[persisted.index("[settings]"):]
        app.process.kill()
        app.process.wait(timeout=3)
        app.close()
        self.app = None
        self.server.release.set()
        app = self.launch(*flags, "--resume", str(path))
        restored = app.record()[1]
        for field in ("model_calls", "tool_calls", "started_ms"):
            self.assertEqual(restored["allocation"][field], saved["allocation"][field])
        self.assertEqual(restored["agents"][1]["identity"], saved["agents"][1]["identity"])
        self.assertIsNone(restored["agents"][1]["worktree"])
        self.assertEqual(len(self.requests("worker")), 1, "restart replayed queued work")
        app.send("/agent-reconcile 1 inspected interrupted worker")
        app.send("/reconcile inspected parent and interrupted worktree")
        app.send("/agents-resume")
        self.wait_agent(2)
        self.assertEqual(self.requests("worker")[-1]["model"], "worker-old")
        app.send("/verify")
        app.send("/review")
        self.assertEqual(self.requests("reviewer")[-1]["model"], "reviewer-new")


if __name__ == "__main__":
    unittest.main(verbosity=2)
