#!/usr/bin/env python3
"""Verify selected credentials and fail-closed login errors using synthetic peers."""
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
import unittest

sys.dont_write_bytecode = True
from terminal_session import BINARY, ROOT, until


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.models.append(body["model"])
        key = self.headers.get("x-api-key") if self.path == "/messages" else self.headers.get("Authorization", "").removeprefix("Bearer ")
        self.server.keys.append(key)
        if self.server.expired or self.server.model_rejected:
            self.send_response(401 if self.server.expired else 400)
            self.end_headers()
            return
        events = ([{"type":"message_start","message":{}}, {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"AUTH-OK"}}, {"type":"message_stop"}]
            if self.path == "/messages" else [{"type":"response.output_text.delta","delta":"AUTH-OK"}, {"type":"response.completed","response":{"output":[]}}])
        data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def run_case(adapter, mode, peer=None):
    with tempfile.TemporaryDirectory(prefix="demoncoder-auth-") as directory:
        root = Path(directory)
        native = adapter.endswith("-api")
        config = root / "settings.toml"
        model = "unsupported-fixture-model" if mode == "unsupported-model" else "fixture-model"
        text = f'default_connection="selected"\n[connections.selected]\nadapter="{adapter}"\nmodel="{model}"\n'
        env = {"PATH":"/usr/bin:/bin", "HOME":str(root), "TERM":"xterm-256color", "LANG":"C.UTF-8",
            "OPENAI_API_KEY":"synthetic-openai-env", "ANTHROPIC_API_KEY":"synthetic-anthropic-env", "CLAUDE_CODE_OAUTH_TOKEN":"synthetic-subscription-login"}
        if native:
            path = "messages" if adapter == "anthropic-api" else "responses"
            text += f'endpoint="http://127.0.0.1:{peer.server_port}/{path}"\n'
            variable = "ANTHROPIC_API_KEY" if adapter == "anthropic-api" else "OPENAI_API_KEY"
            if mode != "missing":
                text += 'api_key="synthetic-saved-key"\n'
            if mode in ("saved", "missing"):
                env.pop(variable)
            expected_key = env.get(variable, "synthetic-saved-key")
            peer.keys = []
            peer.models = []
            peer.expired = mode == "expired"
            peer.model_rejected = mode == "unsupported-model"
            # These login-shaped files must not become an API credential source.
            for folder in (".codex", ".claude"):
                (root / folder).mkdir()
                (root / folder / "auth.json").write_text('{"token":"synthetic-subscription-only"}')
        else:
            text += f'binary="{ROOT / "tests/auth_fixture.py"}"\n'
            (root / "auth-mode").write_text(mode)
        config.write_text(text)
        config.chmod(0o600)
        log = root / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
        process = subprocess.Popen([str(BINARY), "--trust-workspace", "--workspace", str(root), "--config", str(config), "--event-log", str(log)], stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        try:
            if native and mode == "missing":
                process.wait(timeout=5)
                assert process.returncode != 0 and not log.exists()
                assert peer.keys == []
                return
            until(master, process, output, b"Prompt")
            os.write(master, b"Check the selected authentication.\r")
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if select.select([master], [], [], .02)[0]:
                    output.extend(os.read(master, 65536))
                rows = [json.loads(line) for line in log.read_text().splitlines(keepends=True) if line.endswith("\n")]
                events = [row["event"] for row in rows]
                endings = [event for event in events if event["type"] == "turn_finished"]
                if endings:
                    break
            else:
                raise AssertionError("authentication case did not finish")
            good = mode in ("valid", "env", "saved")
            assert endings[-1]["status"] == ("complete" if good else "failed"), (adapter, mode, events)
            assert not any(event["type"].startswith("tool_") for event in events)
            assert not (root / "unauth-effect").exists()
            if good:
                until(master, process, output, b"AUTH-OK")
            else:
                assert any(event["type"] == "error" for event in events)
                assert not any(event["type"] == "text" and "AUTH-OK" in event["text"] for event in events)
            if native:
                assert peer.keys == [expected_key], "wrong credential or an automatic retry/fallback"
                assert peer.models == [model], "model changed or was silently retried"
            else:
                requests = [json.loads(line) for line in (root / "auth-messages.jsonl").read_text().splitlines()]
                if adapter == "codex":
                    assert sum(row.get("method") == "account/read" for row in requests) == 1
                    assert sum(row.get("method") == "turn/start" for row in requests) == int(good)
                    if mode == "unsupported-model":
                        assert [row["params"]["model"] for row in requests if row.get("method") == "thread/start"] == [model]
                else:
                    assert sum(row.get("type") == "user" for row in requests) == 1
            for secret in ("synthetic-openai-env", "synthetic-anthropic-env", "synthetic-saved-key", "synthetic-subscription-login"):
                assert secret not in log.read_text() and secret.encode() not in output
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)


class Authentication(unittest.TestCase):
    def test_native_api_sources_and_errors(self):
        peer = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
        thread = threading.Thread(target=peer.serve_forever, daemon=True)
        thread.start()
        try:
            for adapter in ("openai-api", "anthropic-api"):
                for mode in ("env", "saved", "expired", "missing"):
                    with self.subTest(adapter=adapter, mode=mode):
                        run_case(adapter, mode, peer)
        finally:
            peer.shutdown()
            peer.server_close()
            thread.join(timeout=2)

    def test_subscription_routes_and_errors(self):
        for adapter in ("codex", "claude"):
            for mode in ("valid", "wrong-route", "expired", "missing", *(("unknown-route", "no-init", "tool-before-init") if adapter == "claude" else ())):
                with self.subTest(adapter=adapter, mode=mode):
                    run_case(adapter, mode)


if __name__ == "__main__":
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Authentication))
    print("cairn: CONN-003: " + ("pass" if result.wasSuccessful() else "fail"))
    raise SystemExit(not result.wasSuccessful())
