#!/usr/bin/env python3
"""Drive actual first startup, private persistence, repeated startup and refusal."""
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import select
import shlex
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
import tomllib
import unittest

sys.dont_write_bytecode = True
from terminal_session import BINARY, FIXTURE, until
from terminal_screen import screen_text

HOME = b"\x1b[H"
END = b"\x1b[F"
DOWN = b"\x1b[B"
ENTER = b"\r"
ESCAPE = b"\x1b"


class CatalogServer:
    """A local authentication/catalog peer; every credential is synthetic."""
    def __init__(self, routes=None):
        self.routes = routes or {"/openai": {"key": "synthetic-onboarding-secret", "models": ["changed-model", "fixture-model"]}}
        self.requests = []
        self.started = threading.Event()
        self.release = threading.Event()
        self.hold_timed_out = False
        owner = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                prefix = self.path.removesuffix("/models")
                profile = owner.routes.get(prefix)
                owner.requests.append({"method": "GET", "path": self.path, "authorization": self.headers.get("Authorization"), "x-api-key": self.headers.get("x-api-key")})
                if profile is None:
                    self.send_error(404)
                    return
                supplied = self.headers.get("x-api-key") if profile.get("adapter") == "anthropic-api" else self.headers.get("Authorization", "").removeprefix("Bearer ")
                time.sleep(profile.get("delay", 0))
                status = profile.get("status", 200 if supplied == profile["key"] else 401)
                body = json.dumps({"data": [{"id": model} for model in profile.get("models", [])]} if status == 200 else {"error": "rejected " + (supplied or "missing credential")}).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                try:
                    self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    pass

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                profile = owner.routes.get(self.path.rsplit("/", 1)[0], {})
                record = {"method": "POST", "path": self.path, "body": body, "authorization": self.headers.get("Authorization"), "x-api-key": self.headers.get("x-api-key"), "completed": False}
                owner.requests.append(record)
                if self.path.endswith("/responses"):
                    prompt = body["input"][-1]["content"]
                    text = profile.get("response_text", "RECEIVED-" + prompt)
                    events = [{"type": "response.output_text.delta", "delta": text}, {"type": "response.completed", "response": {"output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}]}}]
                    waiting = {"type": "response.output_text.delta", "delta": "WAITING-" + prompt + "\n"}
                else:
                    prompt = body["messages"][-1]["content"]
                    events = [{"type": "message_start", "message": {"usage": {"input_tokens": 1}}}, {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "RECEIVED-" + prompt}}, {"type": "message_stop"}]
                    waiting = {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "WAITING-" + prompt + "\n"}}
                hold = profile.get("hold_first", False) and not owner.started.is_set()
                prefix = ("data: " + json.dumps(waiting) + "\n\n").encode() if hold else b""
                data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(prefix) + len(data)))
                self.end_headers()
                try:
                    if hold:
                        self.wfile.write(prefix)
                        self.wfile.flush()
                        owner.started.set()
                        owner.hold_timed_out = not owner.release.wait(timeout=30)
                    self.wfile.write(data)
                    self.wfile.flush()
                    record["completed"] = True
                except (BrokenPipeError, ConnectionResetError):
                    pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=lambda: self.server.serve_forever(poll_interval=0.02), daemon=True)
        self.thread.start()

    def endpoint(self, prefix="/openai", adapter="openai-api"):
        return f"http://127.0.0.1:{self.server.server_port}{prefix}/" + ("messages" if adapter == "anthropic-api" else "responses")

    def close(self):
        self.release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class App:
    def __init__(self, root, workspace="project", setup=False, omit_workspace=False, config=None, arguments=(), preconfig=None, environment=None, backend_profiles=None, backends=("codex", "claude")):
        self.root = root
        self.home = root / "home"
        self.home.mkdir(exist_ok=True)
        if preconfig is not None:
            self.settings.parent.mkdir(exist_ok=True, mode=0o700)
            self.settings.write_text(preconfig)
            self.settings.chmod(0o600)
        self.backend_requests = root / "backend-requests.jsonl"
        (self.home / "backend-probe-fixture.json").write_text(json.dumps({"requests": str(self.backend_requests), **(backend_profiles or {})}))
        self.workspace = root / workspace
        self.workspace.mkdir(exist_ok=True)
        subprocess.run(["git", "init", "-q", str(self.workspace)], check=True)
        binary_dir = root / "bin"
        binary_dir.mkdir(exist_ok=True)
        for name in backends:
            backend = binary_dir / name
            backend.write_text(f'#!/bin/sh\nexec /usr/bin/python3 {shlex.quote(str(FIXTURE))} "$@"\n')
            backend.chmod(0o700)
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
        self.initial_flags = termios.tcgetattr(slave)
        self.log = root / (workspace + "-" + str(len(list(root.glob("*.jsonl")))) + ".jsonl")
        self.process = subprocess.Popen([str(BINARY), *arguments, *(["--setup"] if setup else []), * ([] if omit_workspace else ["--workspace", str(self.workspace)]), * ([] if config is None else ["--config", str(config)]), "--event-log", str(self.log)],
            stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
            cwd=self.workspace if omit_workspace else root,
            env={"PATH":str(binary_dir) + ":/usr/bin:/bin", "HOME":str(self.home), "TERM":"xterm-256color", "LANG":"C.UTF-8", **(environment or {})})
        os.close(slave)
        self.output = bytearray()

    def answer(self, prompt, text=""):
        self.wait(prompt)
        os.write(self.master, text.encode() + b"\r")

    def wait(self, text):
        until(self.master, self.process, self.output, text.encode())

    def send(self, keys):
        os.write(self.master, keys.encode() if isinstance(keys, str) else keys)

    def resize(self, rows, columns):
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)

    def wait_current(self, text, timeout=8):
        deadline = time.monotonic() + timeout
        while True:
            rows, columns, _, _ = struct.unpack("HHHH", fcntl.ioctl(self.master, termios.TIOCGWINSZ, b"\0" * 8))
            screen = screen_text(self.output, max(1, columns), max(1, rows))
            if text in screen:
                # A ratatui frame can span several PTY writes. Finish reading
                # that frame before returning a screen used for row assertions.
                while select.select([self.master], [], [], 0.02)[0]:
                    try:
                        data = os.read(self.master, 65536)
                    except OSError:
                        break
                    if not data:
                        break
                    self.output.extend(data)
                screen = screen_text(self.output, max(1, columns), max(1, rows))
                if text in screen:
                    return screen
            if self.process.poll() is not None:
                raise AssertionError("application exited before current screen: " + text + "\n" + screen)
            if time.monotonic() >= deadline:
                raise AssertionError("current screen timed out: " + text + "\n" + screen)
            if select.select([self.master], [], [], 0.05)[0]:
                self.output.extend(os.read(self.master, 65536))

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
    app.wait_current("Space toggles")
    app.send(HOME + DOWN * (2 if provider == "1" else 3) + b" ")
    if key is not None:
        app.wait_current("API key: Enter checks")
        app.send(key + "\r")
    app.wait_current("Authenticated")
    app.send(END + ENTER)
    app.wait_current("Enter assigns")
    app.wait_current("fixture-model")
    app.send(END + ENTER)
    app.wait_current("Settings · Agent assignments")
    app.send(END + ENTER)
    app.wait("Prompt")


class Onboarding(unittest.TestCase):
    def test_first_run_and_repeated_run_use_saved_choices(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-onboard-") as directory:
            root = Path(directory)
            app = App(root, arguments=("--effort", "medium"))
            try:
                complete_setup(app)
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(app.settings.stat().st_mode & 0o777, 0o600)
                self.assertTrue(settings["onboarding_complete"])
                self.assertEqual(settings["default_connection"], "codex")
                self.assertEqual(settings["connections"]["codex"]["model"], "fixture-model")
                self.assertEqual(settings["connections"]["codex"]["effort"], "medium")
                self.assertEqual(settings["settings"]["creator"]["connection"], "codex")
                self.assertEqual(settings["settings"]["overrides"], {})
                self.assertEqual(settings["trusted_workspaces"], [str(app.workspace)])
                os.write(app.master, b"first-start\r")
                app.wait("RECEIVED-first-start")
                app.finish()
                before = app.settings.read_bytes()
            finally:
                app.close()

            app = App(root, setup=True, arguments=("--effort", "high"))
            try:
                app.wait_current("Space toggles")
                app.send(HOME + DOWN * 2 + b"r")
                app.wait_current("Authenticated")
                app.send(END + ENTER)
                app.wait_current("Settings · Agent assignments")
                app.send(HOME + ENTER)
                app.wait_current("Enter assigns")
                app.send(HOME + ENTER)
                app.wait_current("Settings · Agent assignments")
                app.send(END + ENTER)
                app.wait("Prompt")
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["connections"]["codex"]["model"], "changed-model")
                self.assertEqual(settings["connections"]["codex"]["effort"], "high")
                self.assertEqual(settings["settings"]["creator"]["model"], "changed-model")
                self.assertEqual(settings["settings"]["overrides"], {})
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
            key = "synthetic-onboarding-secret"
            server = CatalogServer()
            preconfig = f'[settings]\nproviders=[]\n[connections.openai-api]\nadapter="openai-api"\nendpoint={json.dumps(server.endpoint())}\n'
            app = App(Path(directory), arguments=("--max-output-tokens", "64000"), preconfig=preconfig)
            try:
                complete_setup(app, provider="3", key=key)
                self.assertNotIn(key.encode(), app.output)
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["connections"]["openai-api"]["api_key"], key)
                self.assertEqual(settings["connections"]["openai-api"]["max_output_tokens"], 64000)
                self.assertEqual(app.settings.stat().st_mode & 0o777, 0o600)
                self.assertNotIn(key, app.log.read_text())
                self.assertTrue(server.requests)
                self.assertTrue(all(request["method"] == "GET" for request in server.requests))
                self.assertEqual(server.requests[0]["authorization"], "Bearer " + key)
                app.finish()
            finally:
                app.close()
                server.close()

    def test_cancelled_key_input_restores_terminal_and_saves_nothing(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-onboard-cancel-") as directory:
            app = App(Path(directory))
            try:
                app.answer("Trust this project for coding tools? [N]:", "y")
                app.wait_current("Space toggles")
                app.send(HOME + DOWN * 3 + b" ")
                app.wait_current("API key: Enter checks")
                app.send(b"synthetic-cancelled-secret" + ESCAPE)
                app.wait_current("Cancelled")
                app.send(ESCAPE)
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
