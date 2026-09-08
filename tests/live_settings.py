#!/usr/bin/env python3
"""Verify live Settings against held provider requests and private saved state."""
import fcntl
import json
from pathlib import Path
import tempfile
import time
import tomllib
import unittest
import sys
sys.dont_write_bytecode = True

from onboarding import App, CatalogServer, DOWN, END, ENTER, ESCAPE, HOME
from provider_agent_settings import backend_requests

ROLES = ("Creator", "Worker", "Oracle", "Reviewer", "Advisor", "Judge")
KEY = "synthetic-live-settings-key"


def open_settings(app, shortcut=True):
    app.send(b"\x13" if shortcut else b"/settings\r")
    return app.wait_current("Settings · Providers")


def check_providers(app, names):
    screen = app.wait_current("Settings · Providers")
    rows = [line for line in screen.splitlines() if "[x]" in line or "[ ]" in line]
    for name in names:
        matches = [index for index, row in enumerate(rows) if f"({name}) — " in row]
        assert len(matches) == 1, "provider is not visible: " + name
        assert "[x]" in rows[matches[0]], "provider must already be selected: " + name
        app.send(HOME + DOWN * matches[0] + b"r")
        app.wait_current(f"({name}) — Authenticated")
    app.send(END + ENTER)
    return app.wait_current("Settings · Agent assignments")


def choose_role(app, role, model=None, connection=None, inherit=False):
    role = role.capitalize()
    app.wait_current("Settings · Agent assignments")
    app.send(HOME + DOWN * ROLES.index(role) + ENTER)
    app.wait_current(f"Settings · {role} model")
    app.send(HOME)
    screen = app.wait_current("Enter assigns")
    if inherit:
        assert role != "Creator"
        index = 0
    else:
        choices = []
        for row in screen.splitlines():
            if " · " not in row or "(" not in row or ")" not in row:
                continue
            label = row.split(" · ", 1)[0].strip().removeprefix("│").strip().removeprefix("›").strip().removeprefix("✓").strip()
            name = row.rsplit("(", 1)[1].split(")", 1)[0]
            choices.append((label, name))
        target = (model, connection)
        assert target in choices, f"requested model is absent: {target!r}; choices={choices!r}"
        index = choices.index(target) + int(role != "Creator")
    app.send(DOWN * index + ENTER)
    return app.wait_current("Settings · Agent assignments")


def save_settings(app, expect_error=None):
    app.send(END + ENTER)
    return app.wait_current(expect_error or "Ctrl-Q quit")


def close_settings(app):
    app.send(ESCAPE)
    app.wait_current("Settings · Providers")
    app.send(ESCAPE)
    return app.wait_current("Ctrl-Q quit")


def live_config(adapter="openai-api", endpoint=None, selected=None):
    selected = selected or [adapter]
    text = f'onboarding_complete=true\ndefault_connection={json.dumps(adapter)}\n'
    for name in selected:
        text += f'[connections.{name}]\nadapter={json.dumps(name)}\nmodel="fixture-model"\n'
        if name.endswith("-api"):
            text += f'endpoint={json.dumps(endpoint)}\napi_key={json.dumps(KEY)}\n'
    text += f'[settings]\nproviders={json.dumps(selected)}\n[settings.creator]\nconnection={json.dumps(adapter)}\nmodel="fixture-model"\n'
    return text


def live_app(root, adapter="openai-api", server=None, arguments=(), selected=None, **options):
    app = App(root, preconfig=live_config(adapter, server.endpoint() if server else None, selected), arguments=("--trust-workspace", *arguments), **options)
    app.wait("Prompt")
    return app


def event_records(app):
    if not app.log.exists():
        return []
    raw = app.log.read_text()
    lines = raw.splitlines()
    if raw and not raw.endswith("\n"):
        lines = lines[:-1]
    return [json.loads(line) for line in lines]


def wait_for(predicate, description, timeout=5):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.01)
    raise AssertionError("timed out waiting for " + description)


def wait_turns(app, count):
    return wait_for(lambda: len([r for r in event_records(app) if r["event"]["type"] == "turn_finished"]) >= count, "completed turn events")


def posts(server):
    return [request for request in server.requests if request["method"] == "POST"]


class LiveSettings(unittest.TestCase):
    def test_held_native_turn_keeps_model_and_next_work_retains_history_and_draft(self):
        server = CatalogServer({"/openai": {"key": KEY, "models": ["changed-model", "fixture-model"], "hold_first": True}})
        with tempfile.TemporaryDirectory(prefix="demoncoder-live-native-") as directory:
            app = live_app(Path(directory), server=server)
            try:
                app.send("held-native\r")
                app.wait("WAITING-held-native")
                self.assertTrue(server.started.is_set())
                app.send("retained-draft")
                open_settings(app)
                app.resize(24, 100)
                check_providers(app, ["openai-api"])
                choose_role(app, "Creator", "changed-model", "openai-api")
                screen = save_settings(app)
                self.assertIn("retained-draft", screen)
                self.assertIn("fixture-model", screen)
                self.assertEqual(tomllib.loads(app.settings.read_text())["settings"]["creator"]["model"], "changed-model")
                self.assertEqual(len(posts(server)), 1)
                self.assertEqual(posts(server)[0]["body"]["model"], "fixture-model")
                self.assertFalse(posts(server)[0]["completed"])
                self.assertFalse(any(r["event"]["type"] == "model_assignment" for r in event_records(app)))
                app.resize(40, 180)
                server.release.set()
                app.wait("RECEIVED-held-native")
                wait_turns(app, 1)
                app.send(ENTER)
                app.wait("RECEIVED-retained-draft")
                wait_turns(app, 2)
                requests = posts(server)
                self.assertEqual([r["body"]["model"] for r in requests], ["fixture-model", "changed-model"])
                history = requests[1]["body"]["input"]
                self.assertEqual([item["content"] for item in history if item.get("role") == "user"], ["held-native", "retained-draft"])
                self.assertTrue(any(item.get("role") == "assistant" and "RECEIVED-held-native" in json.dumps(item) for item in history))
                assignments = [r["event"] for r in event_records(app) if r["event"]["type"] == "model_assignment"]
                self.assertEqual(len(assignments), 1)
                self.assertIn("Native conversation retained", assignments[0]["explanation"])
                self.assertFalse(server.hold_timed_out)
                app.finish()
            finally:
                server.release.set()
                app.close()
                server.close()

    def test_held_cli_turn_finishes_before_new_context_and_settings_command_is_not_a_prompt(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-live-cli-") as directory:
            root = Path(directory)
            release = root / "release-cli"
            app = live_app(root, adapter="codex", backend_profiles={"codex": {"hold_prompt": "held-cli", "release": str(release)}})
            try:
                app.send("held-cli\r")
                app.wait("WAITING-held-cli")
                open_settings(app, shortcut=False)
                check_providers(app, ["codex"])
                choose_role(app, "Creator", "changed-model", "codex")
                save_settings(app)
                requests = backend_requests(app)
                old_turns = [r for r in requests if r["message"].get("method") == "turn/start"]
                self.assertEqual(len(old_turns), 1)
                self.assertEqual(old_turns[0]["message"]["params"]["input"][0]["text"], "held-cli")
                self.assertFalse(any(r["event"]["type"] == "turn_finished" for r in event_records(app)))
                release.touch()
                app.wait("RECEIVED-held-cli")
                wait_turns(app, 1)
                app.send("cli-next\r")
                app.wait("RECEIVED-cli-next")
                wait_turns(app, 2)
                requests = backend_requests(app)
                threads = [r for r in requests if r["message"].get("method") == "thread/start"]
                self.assertEqual([r["message"]["params"]["model"] for r in threads], ["fixture-model", "changed-model"])
                self.assertNotEqual(threads[0]["pid"], threads[1]["pid"])
                turns = [r for r in requests if r["message"].get("method") == "turn/start"]
                self.assertEqual([r["message"]["params"]["input"][0]["text"] for r in turns], ["held-cli", "cli-next"])
                notices = [r["event"] for r in event_records(app) if r["event"]["type"] == "model_assignment"]
                self.assertEqual(len(notices), 1)
                self.assertIn("new provider context", notices[0]["explanation"])
                self.assertIn("no opaque backend session was transferred", notices[0]["explanation"])
                screen = app.wait_current("RECEIVED-cli-next")
                self.assertIn("RECEIVED-held-cli", screen)
                app.finish()
            finally:
                release.touch()
                app.close()


    def test_explicit_launch_model_overrides_later_saved_creator_changes(self):
        server = CatalogServer({"/openai": {"key": KEY, "models": ["changed-model", "fixture-model"]}})
        with tempfile.TemporaryDirectory(prefix="demoncoder-live-override-") as directory:
            app = live_app(Path(directory), server=server, arguments=("--model", "launch-model"))
            try:
                app.send("before-override\r")
                app.wait("RECEIVED-before-override")
                wait_turns(app, 1)
                app.send("after-override")
                screen = open_settings(app)
                self.assertIn("explicit launch/assignment overrides still take precedence", screen)
                check_providers(app, ["openai-api"])
                choose_role(app, "Creator", "changed-model", "openai-api")
                screen = save_settings(app)
                self.assertIn("after-override", screen)
                self.assertIn("launch-model", screen)
                self.assertEqual(tomllib.loads(app.settings.read_text())["settings"]["creator"]["model"], "changed-model")
                app.send(ENTER)
                app.wait("RECEIVED-after-override")
                wait_turns(app, 2)
                self.assertEqual([request["body"]["model"] for request in posts(server)], ["launch-model", "launch-model"])
                self.assertFalse(any(r["event"]["type"] == "model_assignment" for r in event_records(app)))
                app.finish()
            finally:
                app.close()
                server.close()

    def test_cancelling_live_slow_discovery_keeps_draft_and_scrolled_conversation(self):
        history = "SCROLL-ANCHOR-BEGIN\n\n" + "\n\n".join(f"history-line-{number:03d}" for number in range(60)) + "\n\nSCROLL-LAST"
        server = CatalogServer({"/openai": {"key": KEY, "models": ["changed-model", "fixture-model"], "response_text": history}})
        with tempfile.TemporaryDirectory(prefix="demoncoder-live-scroll-") as directory:
            app = live_app(Path(directory), server=server)
            try:
                app.send("create-history\r")
                app.wait_current("SCROLL-LAST")
                wait_turns(app, 1)
                app.send("scroll-draft")
                app.send(b"\x0f" + HOME)
                before = app.wait_current("history-line-010")
                self.assertIn("SCROLL-ANCHOR-BEGIN", before)
                visible_history = [line.strip() for line in before.splitlines() if "history-line-" in line or "SCROLL-" in line]
                self.assertTrue(visible_history)
                self.assertNotIn("SCROLL-LAST", before)
                saved = app.settings.read_bytes()
                server.routes["/openai"]["delay"] = 20
                open_settings(app)
                app.send(HOME + DOWN * 3 + b"r")
                app.wait_current("Checking")
                app.send(ESCAPE)
                app.wait_current("Cancelled · press r to retry")
                app.send(ESCAPE)
                screen = app.wait_current("Ctrl-Q quit")
                self.assertIn("scroll-draft", screen)
                self.assertEqual([line.strip() for line in screen.splitlines() if "history-line-" in line or "SCROLL-" in line], visible_history)
                self.assertEqual(app.settings.read_bytes(), saved)
                self.assertEqual(len(posts(server)), 1)
                self.assertFalse(any(r["event"]["type"] == "model_assignment" for r in event_records(app)))
                app.finish()
            finally:
                app.close()
                server.close()

    def test_deselected_creator_blocks_admission_without_redirecting_or_losing_draft(self):
        server = CatalogServer({"/openai": {"key": KEY, "models": ["changed-model", "fixture-model"]}})
        with tempfile.TemporaryDirectory(prefix="demoncoder-live-deselected-") as directory:
            app = live_app(Path(directory), server=server, selected=["codex", "openai-api"])
            try:
                app.send("before-deselection\r")
                app.wait("RECEIVED-before-deselection")
                wait_turns(app, 1)
                app.send("blocked-draft")
                open_settings(app)
                app.send(HOME + DOWN * 3 + b" ")
                app.wait_current("OpenAI API (openai-api) — Not selected")
                screen = check_providers(app, ["codex"])
                self.assertIn("Unavailable", screen)
                screen = save_settings(app)
                self.assertIn("blocked-draft", screen)
                saved = tomllib.loads(app.settings.read_text())["settings"]
                self.assertEqual(saved["providers"], ["codex"])
                self.assertEqual(saved["creator"]["connection"], "openai-api")
                app.send(ENTER)
                screen = app.wait_current("Assignment unavailable; draft retained")
                self.assertIn("blocked-draft", screen)
                self.assertEqual(len(posts(server)), 1)
                self.assertFalse(any(r["message"].get("method") == "turn/start" for r in backend_requests(app)))
                app.finish()
            finally:
                app.close()
                server.close()

    def test_conflicting_and_failed_saves_stay_open_and_keep_previous_runtime_model(self):
        for failure in ("conflict", "lock"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory(prefix="demoncoder-live-save-") as directory:
                server = CatalogServer({"/openai": {"key": KEY, "models": ["changed-model", "fixture-model"]}})
                app = live_app(Path(directory), server=server)
                lock = None
                try:
                    app.send("before-save-failure\r")
                    app.wait("RECEIVED-before-save-failure")
                    wait_turns(app, 1)
                    app.send("after-" + failure)
                    open_settings(app)
                    check_providers(app, ["openai-api"])
                    choose_role(app, "Creator", "changed-model", "openai-api")
                    expected = app.settings.read_bytes()
                    if failure == "conflict":
                        expected += b"\n# a competing editor must not lose this change\n"
                        app.settings.write_bytes(expected)
                    else:
                        lock_path = app.settings.with_name(app.settings.name + ".lock")
                        lock = lock_path.open("a")
                        lock_path.chmod(0o600)
                        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    screen = save_settings(app, expect_error="Not applied:")
                    self.assertIn("Settings · Agent assignments", screen)
                    self.assertIn("settings changed in another editor" if failure == "conflict" else "another setup is updating these settings", screen)
                    self.assertEqual(app.settings.read_bytes(), expected)
                    self.assertEqual(len(posts(server)), 1)
                    if lock is not None:
                        lock.close()
                        lock = None
                    screen = close_settings(app)
                    self.assertIn("after-" + failure, screen)
                    app.send(ENTER)
                    app.wait("RECEIVED-after-" + failure)
                    wait_turns(app, 2)
                    self.assertEqual([request["body"]["model"] for request in posts(server)], ["fixture-model", "fixture-model"])
                    self.assertEqual(app.settings.read_bytes(), expected)
                    app.finish()
                finally:
                    if lock is not None:
                        lock.close()
                    app.close()
                    server.close()


if __name__ == "__main__":
    unittest.main(verbosity=2)
