#!/usr/bin/env python3
"""Check the actual chat viewport, rather than historical PTY output bytes."""
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
import unittest

sys.dont_write_bytecode = True
from provider_metadata import ModelMetadataHandler
from terminal_screen import screen_text
from terminal_session import BINARY


class Provider(ModelMetadataHandler):
    def log_message(self, *_):
        pass

    def event(self, value):
        self.wfile.write(("data: " + json.dumps(value) + "\n\n").encode())
        self.wfile.flush()

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        prompt = body["input"][-1]["content"]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            if prompt == "large-scrollback":
                text = "".join(f"ROW-{index:05d}\n" for index in range(18000)) + "EXPIRED-END\n"
                self.event({"type": "response.output_text.delta", "delta": text})
            else:
                assert prompt == "scroll-check"
                text = "".join(f"ROW-{index:05d} " + "x" * 60 + "\n" for index in range(400))
                self.event({"type": "response.output_text.delta", "delta": text})
                assert self.server.more.wait(12), "test did not release streaming output"
                extra = "".join(f"ROW-{index:05d} NEW\n" for index in range(400, 450)) + "NEWEST-MARKER\n"
                self.event({"type": "response.output_text.delta", "delta": extra})
                text += extra
            self.event({"type": "response.completed", "response": {
                "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}],
                "usage": {"input_tokens": 12345, "output_tokens": 9},
            }})
        except (BrokenPipeError, ConnectionResetError):
            pass


class App:
    def __init__(self, root, provider=Provider, prepare=None, adapter="openai-api", connection_settings="", arguments=()):
        self.root = root
        self.workspace = root / "workspace"
        self.workspace.mkdir()
        home = root / "home"
        home.mkdir()
        if prepare:
            prepare(self.workspace, home)
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), provider)
        self.server.daemon_threads = True
        self.server.more = threading.Event()
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        config = root / "connection.toml"
        route = "messages" if adapter == "anthropic-api" else "responses"
        config.write_text(f'default_connection="fixture"\n[connections.fixture]\nadapter="{adapter}"\nmodel="fixture-model"\nendpoint="http://127.0.0.1:{self.server.server_port}/{route}"\n' + connection_settings)
        self.master, slave = pty.openpty()
        self.rows, self.columns = 35, 100
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", self.rows, self.columns, 0, 0))
        env = {"PATH": "/usr/bin:/bin", "HOME": str(home), "TERM": "xterm-256color", "LANG": "C.UTF-8", "OPENAI_API_KEY": "synthetic-key", "ANTHROPIC_API_KEY": "synthetic-key"}
        self.process = subprocess.Popen([str(BINARY), "--trust-workspace", "--workspace", str(self.workspace), "--config", str(config), *arguments],
                                        stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        self.output = bytearray()
        self.wait(lambda screen: "Prompt" in screen, "initial prompt")

    def collect(self, timeout=.03):
        if select.select([self.master], [], [], timeout)[0]:
            try:
                self.output.extend(os.read(self.master, 65536))
            except OSError:
                pass

    def screen(self):
        return screen_text(self.output, self.columns, self.rows)

    def wait(self, predicate, description, timeout=8):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            self.collect()
            current = self.screen()
            if predicate(current):
                return current
            assert self.process.poll() is None, "app exited before " + description
        raise AssertionError("visible screen did not reach " + description + "\n" + self.screen())

    def send(self, value):
        os.write(self.master, value)

    def resize(self, rows, columns):
        self.rows, self.columns = rows, columns
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)
        for _ in range(5):
            self.collect(.04)

    def close(self):
        self.server.more.set()
        if self.process.poll() is None:
            self.send(b"\x11")
            self.process.wait(timeout=5)
        self.collect(0)
        os.close(self.master)
        self.server.shutdown()
        self.server.server_close()


def row_numbers(screen):
    return [int(number) for number in re.findall(r"ROW-(\d{5})", screen)]


class Scrollback(unittest.TestCase):
    def test_page_mouse_resize_and_new_output_preserve_history(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-scrollback-") as directory:
            app = App(Path(directory))
            try:
                app.send(b"\x0fscroll-check\r")
                app.wait(lambda screen: "ROW-00399" in screen, "initial live tail")
                app.send(b"\x1b[5~")
                older = app.wait(lambda screen: bool(row_numbers(screen)) and max(row_numbers(screen)) < 399, "Page Up showing earlier output")
                first = min(row_numbers(older))
                self.assertGreater(first, 0)
                app.send(b"still-responsive")
                app.wait(lambda screen: "still-responsive" in screen, "input while reading streaming history")
                app.resize(40, 60)
                resized = app.wait(lambda screen: bool(row_numbers(screen)) and min(row_numbers(screen)) == first, "history anchor after resize")
                self.assertNotIn("ROW-00399", resized)
                app.send(b"\x1b[<64;5;5M")
                wheeled = app.wait(lambda screen: bool(row_numbers(screen)) and min(row_numbers(screen)) < first, "mouse wheel scrolling")
                first = min(row_numbers(wheeled))
                app.server.more.set()
                completed = app.wait(lambda screen: "in 12345" in screen, "new output and completed usage")
                self.assertEqual(min(row_numbers(completed)), first)
                self.assertNotIn("NEWEST-MARKER", completed)
                app.send(b"\x1b[F")
                app.wait(lambda screen: "NEWEST-MARKER" in screen, "End returning to latest output")
                app.send(b"\x1b[H")
                app.wait(lambda screen: "ROW-00000" in screen, "Home returning to oldest retained output")
                app.send(b"\x1b[6~")
                app.wait(lambda screen: bool(row_numbers(screen)) and min(row_numbers(screen)) > 0, "Page Down advancing history")
                app.resize(4, 8)
                for key in (b"\x1b[H", b"\x1b[6~", b"\x1b[F"):
                    app.send(key)
                    for _ in range(3):
                        app.collect(.04)
                app.resize(35, 100)
                app.wait(lambda screen: "NEWEST-MARKER" in screen, "restoring a usable viewport after a tiny terminal")
            finally:
                app.close()
            self.assertIn(b"\x1b[?1006h", app.output)
            self.assertIn(b"\x1b[?1006l", app.output)

    def test_history_expiry_is_visible_and_retained_output_is_scrollable(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-expiry-") as directory:
            app = App(Path(directory))
            try:
                app.send(b"\x0flarge-scrollback\r")
                app.wait(lambda screen: "EXPIRED-END" in screen and "Older chat expired" in screen, "bounded-history notice and live tail")
                app.send(b"\x1b[H")
                oldest = app.wait(lambda screen: bool(row_numbers(screen)) and max(row_numbers(screen)) < 17999, "oldest retained history")
                self.assertGreater(min(row_numbers(oldest)), 0)
                self.assertIn("Older chat expired", oldest)
                app.send(b"\x1b[F")
                app.wait(lambda screen: "EXPIRED-END" in screen, "latest retained history")
            finally:
                app.close()


if __name__ == "__main__":
    unittest.main()
