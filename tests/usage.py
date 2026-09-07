#!/usr/bin/env python3
"""Inspect current terminal usage and attributed retained events for all adapters."""
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
from provider_metadata import ModelMetadataHandler
from terminal_session import ROOT, BINARY, until
from terminal_screen import screen_text
from usage_fixture import reported


class Provider(ModelMetadataHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        anthropic = self.path == "/messages"
        mode = (body["messages"] if anthropic else body["input"])[-1]["content"]
        usage = reported("anthropic-api" if anthropic else "openai-api", mode)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()

        def emit(value):
            self.wfile.write(("data: " + json.dumps(value) + "\n\n").encode())
            self.wfile.flush()

        if anthropic:
            emit({"type":"message_start", "message":{"usage":{k:v for k,v in usage.items() if k != "output_tokens"}}})
            emit({"type":"content_block_delta", "index":0, "delta":{"type":"text_delta", "text":"USAGE-WAIT-" + mode}})
        else:
            emit({"type":"response.output_text.delta", "delta":"USAGE-WAIT-" + mode})
        if not self.server.release.wait(15):
            return
        if anthropic:
            emit({"type":"message_delta", "usage":{k:v for k,v in usage.items() if k == "output_tokens"}})
            emit({"type":"message_stop"})
        else:
            usage = dict(usage)
            if "cache_read_input_tokens" in usage:
                usage["input_tokens_details"] = {"cached_tokens":usage.pop("cache_read_input_tokens")}
            emit({"type":"response.completed", "response":{"output":[], "usage":usage}})


def current_footer(master, process, output, expected, row=-2):
    """Require the current screen; old terminal bytes must never satisfy this check."""
    deadline = time.monotonic() + 3
    inspected = -1
    footer = ""
    while time.monotonic() < deadline:
        if len(output) != inspected:
            inspected = len(output)
            footer = screen_text(output, 180, 40).splitlines()[row]
            if expected in footer:
                return
        assert process.poll() is None, "application exited during usage check"
        if select.select([master], [], [], .02)[0]:
            output.extend(os.read(master, 65536))
    raise AssertionError(f"current footer does not contain {expected!r}: {footer!r}")


def run_case(adapter, peer):
    with tempfile.TemporaryDirectory(prefix="demoncoder-usage-") as directory:
        root = Path(directory)
        name = "usage-" + adapter
        config = root / "settings.toml"
        settings = f'onboarding_complete=true\ndefault_connection="{name}"\n[connections.{name}]\nadapter="{adapter}"\nmodel="fixture-model"\n'
        if adapter.endswith("-api"):
            route = "messages" if adapter == "anthropic-api" else "responses"
            settings += f'endpoint="http://127.0.0.1:{peer.server_port}/{route}"\napi_key="synthetic-usage-key"\n'
        else:
            settings += f'binary="{ROOT / "tests/usage_fixture.py"}"\n'
        config.write_text(settings)
        config.chmod(0o600)
        log = root / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
        env = {"PATH":"/usr/bin:/bin", "HOME":str(root), "TERM":"xterm-256color", "LANG":"C.UTF-8"}
        process = subprocess.Popen([str(BINARY), "--trust-workspace", "--workspace", str(root), "--config", str(config), "--event-log", str(log)], stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        try:
            until(master, process, output, b"Prompt")
            current_footer(master, process, output, "└", row=-2)
            current_footer(master, process, output, "Ctrl-Q quit", row=-1)
            offset = {"openai-api":0, "anthropic-api":1, "codex":2, "claude":3}[adapter]
            for mode in ("known", "partial", "zero", "absent"):
                peer.release.clear()
                os.write(master, mode.encode() + b"\r")
                until(master, process, output, ("USAGE-WAIT-" + mode).encode())
                current_footer(master, process, output, "└", row=-2)
                current_footer(master, process, output, "Ctrl-Q quit", row=-1)
                peer.release.set()
                (root / ("release-" + mode)).touch()
                if mode == "known":
                    expected = {"input":11 + offset, "output":7 + offset, "cached":2 + offset, "cost_usd":.0123 if adapter == "claude" else None}
                elif mode == "partial":
                    expected = {"input":0, "output":None, "cached":None, "cost_usd":None}
                elif mode == "zero":
                    expected = {"input":0, "output":0, "cached":0, "cost_usd":0.0 if adapter == "claude" else None}
                else:
                    expected = {"input":None, "output":None, "cached":None, "cost_usd":None}
                count = lambda value: "unknown" if value is None else str(value)
                cost = "unknown" if expected["cost_usd"] is None else f"${expected['cost_usd']:.4f}"
                expected_text = f"in {count(expected['input'])} · out {count(expected['output'])} · cached {count(expected['cached'])} · cost {cost}"
                if mode == "absent":
                    expected_text = "└"
                current_footer(master, process, output, "complete", row=0)
                current_footer(master, process, output, expected_text)
                screen = screen_text(output, 180, 40)
                assert name in screen, "selected connection is missing from the screen"
                rows = [json.loads(line) for line in log.read_text().splitlines()]
                assert all(row["connection"] == name for row in rows), "misattributed connection usage"
                events = [row["event"] for row in rows]
                start = max(i for i,e in enumerate(events) if e["type"] == "turn_started")
                turn = events[start:]
                assert turn[-1] == {"type":"turn_finished", "status":"complete"}
                usages = [e for e in turn if e["type"] == "usage"]
                assert usages == ([] if adapter == "codex" and mode == "absent" else [{"type":"usage", **expected}]), (adapter, mode, usages)
                assert not any(e["type"].startswith("tool_") for e in turn)
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0
        finally:
            peer.release.set()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)


class Usage(unittest.TestCase):
    def test_reported_partial_zero_and_absent_usage(self):
        peer = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
        peer.release = threading.Event()
        thread = threading.Thread(target=peer.serve_forever, daemon=True)
        thread.start()
        try:
            for adapter in ("openai-api", "anthropic-api", "codex", "claude"):
                with self.subTest(adapter=adapter):
                    run_case(adapter, peer)
        finally:
            peer.release.set()
            peer.shutdown()
            peer.server_close()
            thread.join(timeout=2)


if __name__ == "__main__":
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Usage))
    print("cairn: CONN-006: " + ("pass" if result.wasSuccessful() else "fail"))
    raise SystemExit(not result.wasSuccessful())
