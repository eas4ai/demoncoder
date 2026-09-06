#!/usr/bin/env python3
"""Exercise the built executable via a PTY and real HTTP/JSONL adapter paths."""
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
import tempfile
import termios
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/demoncoder"
FIXTURE = ROOT / "tests/backend_fixture.py"


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path == "/responses":
            assert self.headers["Authorization"] == "Bearer synthetic-openai-key"
            assert "x-api-key" not in self.headers
            prompt = body["input"][-1]["content"]
            text = "RECEIVED-" + prompt
            events = [
                {"type": "response.output_text.delta", "delta": text},
                {"type": "response.completed", "response": {"output": [{"role": "assistant", "type": "message", "content": [{"type": "output_text", "text": text}]}], "usage": {"input_tokens": 10, "output_tokens": 8}}},
            ]
        else:
            assert self.path == "/messages"
            assert self.headers["x-api-key"] == "synthetic-anthropic-key"
            assert "Authorization" not in self.headers
            prompt = body["messages"][-1]["content"]
            text = "RECEIVED-" + prompt
            events = [
                {"type": "message_start", "message": {"usage": {"input_tokens": 11}}},
                {"type": "content_block_delta", "delta": {"type": "text_delta", "text": text}},
                {"type": "message_delta", "usage": {"output_tokens": 8}},
                {"type": "message_stop"},
            ]
        self.server.received.append(prompt)
        data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        self.wfile.flush()


def until(master, process, output, needle, timeout=8):
    deadline = time.monotonic() + timeout
    while needle not in output:
        if process.poll() is not None:
            raise AssertionError("application exited before terminal checkpoint: " + needle.decode())
        if time.monotonic() >= deadline:
            raise AssertionError("terminal checkpoint timed out: " + needle.decode())
        if select.select([master], [], [], 0.05)[0]:
            try:
                output.extend(os.read(master, 65536))
            except OSError as error:
                raise AssertionError("PTY closed before checkpoint") from error


def case(adapter, server, fault):
    with tempfile.TemporaryDirectory(prefix="demoncoder-terminal-") as directory:
        workspace = Path(directory)
        subprocess.run(["git", "init", "-q", str(workspace)], check=True)
        token = "prompt" + uuid.uuid4().hex[:12]
        config = workspace / "connection.toml"
        settings = f'default_connection = "selected"\n[connections.selected]\nadapter = "{adapter}"\nmodel = "fixture-model"\n'
        if adapter in ("openai-api", "anthropic-api"):
            route = "responses" if adapter == "openai-api" else "messages"
            settings += f'endpoint = "http://127.0.0.1:{server.server_port}/{route}"\n'
        else:
            settings += f'binary = {json.dumps(str(FIXTURE))}\n'
        config.write_text(settings)
        if fault and adapter == "codex":
            (workspace / "drop-prompt").touch()
        log = workspace / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 160, 0, 0))
        env = {"PATH": "/usr/bin:/bin", "HOME": str(workspace), "TERM": "xterm-256color", "LANG": "C.UTF-8",
               "OPENAI_API_KEY": "synthetic-openai-key", "ANTHROPIC_API_KEY": "synthetic-anthropic-key"}
        process = subprocess.Popen([str(BINARY), "--workspace", str(workspace), "--config", str(config), "--event-log", str(log)],
                                   stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        try:
            until(master, process, output, b"Prompt")
            os.write(master, token.encode() + b"\r")
            until(master, process, output, ("RECEIVED-" + token).encode())
            if adapter in ("openai-api", "anthropic-api"):
                assert token in server.received, "selected API runtime did not receive the terminal prompt"
            else:
                assert json.loads((workspace / "received-prompt.json").read_text()) == token
            records = [json.loads(line) for line in log.read_text().splitlines()]
            assert all(record["connection"] == "selected" for record in records)
            assert any(record["event"]["type"] == "turn_started" for record in records)
            assert "synthetic-openai-key" not in log.read_text()
            assert "synthetic-anthropic-key" not in log.read_text()
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0, "application failed during normal shutdown"
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fault-drop-prompt", action="store_true")
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    server.received = []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    failed = []
    try:
        for adapter in ["openai-api", "anthropic-api", "codex", "claude"]:
            try:
                case(adapter, server, args.fault_drop_prompt)
                print("CODE-001", adapter, "terminal prompt reached runtime and response was rendered", flush=True)
            except (AssertionError, OSError, subprocess.SubprocessError) as error:
                failed.append(adapter)
                print("CODE-001", adapter, "FAILED:", str(error), flush=True)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
    print("cairn: CODE-001: " + ("fail" if failed else "pass"))
    return int(bool(failed))


if __name__ == "__main__":
    raise SystemExit(main())
