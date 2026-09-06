#!/usr/bin/env python3
"""Drive actual first startup, private persistence, repeated startup and refusal."""
import fcntl
import json
import os
from pathlib import Path
import pty
import struct
import subprocess
import sys
import tempfile
import termios
import tomllib
import unittest

sys.dont_write_bytecode = True
from terminal_session import BINARY, FIXTURE, until


class App:
    def __init__(self, root, workspace="project", setup=False):
        self.root = root
        self.home = root / "home"
        self.home.mkdir(exist_ok=True)
        self.workspace = root / workspace
        self.workspace.mkdir(exist_ok=True)
        subprocess.run(["git", "init", "-q", str(self.workspace)], check=True)
        binary_dir = root / "bin"
        binary_dir.mkdir(exist_ok=True)
        backend = binary_dir / "codex"
        backend.write_text(f'#!/bin/sh\nexec /usr/bin/python3 {str(FIXTURE)!r} "$@"\n')
        backend.chmod(0o700)
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
        self.initial_flags = termios.tcgetattr(slave)
        self.log = root / (workspace + "-" + str(len(list(root.glob("*.jsonl")))) + ".jsonl")
        self.process = subprocess.Popen([str(BINARY), *(["--setup"] if setup else []), "--workspace", str(self.workspace), "--event-log", str(self.log)],
            stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
            env={"PATH":str(binary_dir) + ":/usr/bin:/bin", "HOME":str(self.home), "TERM":"xterm-256color", "LANG":"C.UTF-8"})
        os.close(slave)
        self.output = bytearray()

    def answer(self, prompt, text=""):
        self.wait(prompt)
        os.write(self.master, text.encode() + b"\r")

    def wait(self, text):
        until(self.master, self.process, self.output, text.encode())

    def finish(self):
        os.write(self.master, b"\x11")
        self.process.wait(timeout=5)
        assert self.process.returncode == 0

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=5)
        os.close(self.master)

    @property
    def settings(self):
        return self.home / ".demoncoder/settings.toml"


def complete_setup(app, provider="1", key=None):
    app.answer("Trust this project for coding tools? [N]:", "y")
    app.answer("Provider [1]:", provider)
    name = "codex" if provider == "1" else "openai-api"
    app.answer(f"Connection name [{name}]:")
    if key is not None:
        app.answer("API key:", key)
    model_default = "backend-default" if provider == "1" else "gpt-6-astra"
    app.answer(f"Model ID [{model_default}]:", "fixture-model")
    app.answer("Thinking/response effort [default]:", "medium")
    app.answer("Add another connection? [N]:", "n")
    app.answer(f"Default connection [{name}]:")
    app.answer(f"Oracle connection for outside access with --yolo [{name}]:")
    app.answer("Oracle model ID [fixture-model]:")
    app.answer("Thinking/response effort [medium]:")
    app.wait("Prompt")


class Onboarding(unittest.TestCase):
    def test_first_run_and_repeated_run_use_saved_choices(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-onboard-") as directory:
            root = Path(directory)
            app = App(root)
            try:
                complete_setup(app)
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(app.settings.stat().st_mode & 0o777, 0o600)
                self.assertTrue(settings["onboarding_complete"])
                self.assertEqual(settings["default_connection"], "codex")
                self.assertEqual(settings["connections"]["codex"]["model"], "fixture-model")
                self.assertEqual(settings["connections"]["codex"]["effort"], "medium")
                self.assertEqual(settings["oracle"]["connection"], "codex")
                self.assertEqual(settings["trusted_workspaces"], [str(app.workspace)])
                os.write(app.master, b"first-start\r")
                app.wait("RECEIVED-first-start")
                app.finish()
                before = app.settings.read_bytes()
            finally:
                app.close()

            app = App(root, setup=True)
            try:
                app.answer("Provider [1]:")
                app.answer("Connection name [codex]:")
                app.answer("Model ID [fixture-model]:", "changed-model")
                app.answer("Thinking/response effort [medium]:", "high")
                app.answer("Add another connection? [N]:", "n")
                app.answer("Default connection [codex]:")
                app.answer("Oracle connection for outside access with --yolo [codex]:")
                app.answer("Oracle model ID [fixture-model]:")
                app.answer("Thinking/response effort [medium]:")
                app.wait("Prompt")
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["connections"]["codex"]["model"], "changed-model")
                self.assertEqual(settings["connections"]["codex"]["effort"], "high")
                self.assertEqual(settings["oracle"]["model"], "fixture-model")
                app.finish()
                before = app.settings.read_bytes()
            finally:
                app.close()
            app = App(root)
            try:
                app.wait("Prompt")
                self.assertNotIn(b"DemonCoder setup", app.output)
                os.write(app.master, b"saved-start\r")
                app.wait("RECEIVED-saved-start")
                app.finish()
                self.assertEqual(app.settings.read_bytes(), before)
            finally:
                app.close()
            app = App(root, "untrusted-project")
            try:
                app.answer("Trust this project for coding tools? [N]:", "n")
                app.process.wait(timeout=5)
                self.assertNotEqual(app.process.returncode, 0)
                self.assertFalse(app.log.exists())
                self.assertEqual(app.settings.read_bytes(), before)
            finally:
                app.close()

    def test_key_input_is_not_echoed_and_is_saved_privately(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-onboard-key-") as directory:
            app = App(Path(directory))
            key = "synthetic-onboarding-secret"
            try:
                complete_setup(app, provider="3", key=key)
                self.assertNotIn(key.encode(), app.output)
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["connections"]["openai-api"]["api_key"], key)
                self.assertEqual(app.settings.stat().st_mode & 0o777, 0o600)
                self.assertNotIn(key, app.log.read_text())
                # No prompt is submitted: this case must never send a synthetic
                # credential to a live provider.
                app.finish()
            finally:
                app.close()

    def test_cancelled_key_input_restores_terminal_and_saves_nothing(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-onboard-cancel-") as directory:
            app = App(Path(directory))
            try:
                app.answer("Trust this project for coding tools? [N]:", "y")
                app.answer("Provider [1]:", "3")
                app.answer("Connection name [openai-api]:")
                app.wait("API key:")
                os.write(app.master, b"synthetic-cancelled-secret\x1b")
                app.process.wait(timeout=5)
                self.assertNotEqual(app.process.returncode, 0)
                self.assertFalse(app.settings.exists())
                self.assertFalse(app.log.exists())
                self.assertEqual(termios.tcgetattr(app.master)[3] & (termios.ECHO | termios.ICANON), app.initial_flags[3] & (termios.ECHO | termios.ICANON))
            finally:
                app.close()


if __name__ == "__main__":
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(Onboarding)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    print("cairn: CODE-009: " + ("pass" if result.wasSuccessful() else "fail"))
    raise SystemExit(not result.wasSuccessful())
