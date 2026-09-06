#!/usr/bin/env python3
"""Observe streaming and editor input while production requests/tools are held."""
import argparse
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import uuid

sys.dont_write_bytecode = True
from terminal_session import BINARY, FIXTURE, until
from tool_cycle_fixture import sse_call


def bash_call(token):
    return {"id": token + "-bash", "name": "bash", "arguments": {
        "command": f"printf 'TOOL-WAIT-{token}\\n'; while ! test -f release-tool; do sleep 0.01; done; printf 'TOOL-DONE\\n'",
    }}


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def emit(self, event):
        self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        history = body["input"] if self.path == "/responses" else body["messages"]
        token = history[0]["content"]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        if len(history) == 1:
            if self.server.buffer_fault:
                # A buffered producer cannot satisfy the pre-release checkpoint.
                if not self.server.release.wait(10):
                    return
            if self.path == "/responses":
                self.emit({"type": "response.output_text.delta", "delta": "ASSISTANT-WAIT-" + token})
            else:
                self.emit({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
                self.emit({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ASSISTANT-WAIT-" + token}})
                self.emit({"type": "content_block_stop", "index": 0})
            if not self.server.release.wait(10):
                return
            for event in sse_call(self.path, bash_call(token)):
                if self.path == "/messages" and "index" in event:
                    event["index"] = 1
                self.emit(event)
        else:
            result = json.loads(history[-1]["output"] if self.path == "/responses" else history[-1]["content"][0]["content"])
            assert result["success"] and "TOOL-DONE" in result["output"]
            if self.path == "/responses":
                self.emit({"type": "response.output_text.delta", "delta": "SESSION-DONE"})
                self.emit({"type": "response.completed", "response": {"output": []}})
            else:
                self.emit({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "SESSION-DONE"}})
                self.emit({"type": "message_stop"})


def case(adapter, server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-responsive-") as directory:
        workspace = Path(directory)
        subprocess.run(["git", "init", "-q", directory], check=True)
        (workspace / "responsiveness").touch()
        if server.buffer_fault:
            (workspace / "buffer-assistant").touch()
        token = uuid.uuid4().hex[:12]
        settings = f'default_connection = "selected"\n[connections.selected]\nadapter = "{adapter}"\nmodel = "fixture-model"\n'
        if adapter in ("openai-api", "anthropic-api"):
            route = "responses" if adapter == "openai-api" else "messages"
            settings += f'endpoint = "http://127.0.0.1:{server.server_port}/{route}"\n'
        else:
            settings += f'binary = {json.dumps(str(FIXTURE))}\n'
        config = workspace / "connection.toml"
        config.write_text(settings)
        log = workspace / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 160, 0, 0))
        env = {"PATH": "/usr/bin:/bin", "HOME": directory, "TERM": "xterm-256color", "LANG": "C.UTF-8", "OPENAI_API_KEY": "fixture-key", "ANTHROPIC_API_KEY": "fixture-key"}
        process = subprocess.Popen([str(BINARY), "--trust-workspace", "--workspace", directory, "--config", str(config), "--event-log", str(log)], stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        try:
            until(master, process, output, b"Prompt")
            os.write(master, token.encode() + b"\r")
            until(master, process, output, ("ASSISTANT-WAIT-" + token).encode(), timeout=3)
            os.write(master, b"draftwhilemodelwaits")
            until(master, process, output, b"draftwhilemodelwaits", timeout=3)
            records = [json.loads(line)["event"] for line in log.read_text().splitlines()]
            assert not any(e["type"] in ("tool_started", "turn_finished") for e in records), "provider checkpoint was already released"
            server.release.set()
            (workspace / "release-provider").touch()
            until(master, process, output, ("TOOL-WAIT-" + token).encode(), timeout=3)
            os.write(master, b"andwhiletoolwaits")
            until(master, process, output, b"andwhiletoolwaits", timeout=3)
            records = [json.loads(line)["event"] for line in log.read_text().splitlines()]
            assert any(e["type"] == "tool_output" for e in records)
            assert not any(e["type"] in ("tool_finished", "turn_finished") for e in records), "tool checkpoint was already released"
            (workspace / "release-tool").touch()
            until(master, process, output, b"SESSION-DONE", timeout=3)
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0
        finally:
            server.release.set()
            (workspace / "release-provider").touch()
            (workspace / "release-tool").touch()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fault-buffer-assistant", action="store_true")
    args = parser.parse_args()
    failed = []
    for adapter in ["openai-api", "anthropic-api", "codex", "claude"]:
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
        server.release = threading.Event()
        server.buffer_fault = args.fault_buffer_assistant
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            case(adapter, server)
            print("CODE-003", adapter, "assistant, tool, and editor checkpoints passed", flush=True)
        except (AssertionError, OSError, subprocess.SubprocessError) as error:
            failed.append(adapter)
            print("CODE-003", adapter, "FAILED:", str(error), flush=True)
        finally:
            server.release.set()
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)
    print("cairn: CODE-003: " + ("fail" if failed else "pass"))
    return int(bool(failed))


if __name__ == "__main__":
    raise SystemExit(main())
