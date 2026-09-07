#!/usr/bin/env python3
"""Cancel real terminal turns; observe HTTP EOF, subprocesses, and workspace activity."""
import argparse
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import select
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
import uuid

sys.dont_write_bytecode = True
from provider_metadata import ModelMetadataHandler
from terminal_session import BINARY, FIXTURE, until
from tool_cycle_fixture import sse_call


def held_tool(token):
    return {"id": token + "-held", "name": "bash", "arguments": {
        "command": f"(while true; do printf x >> heartbeat; sleep 0.02; done) & printf 'CANCELWAIT-{token}\\n'; wait",
    }}


class Provider(ModelMetadataHandler):
    def log_message(self, *_):
        pass

    def emit(self, event):
        self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()

    def text_event(self, text):
        if self.path == "/responses":
            return {"type": "response.output_text.delta", "delta": text}
        return {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        history = body["input"] if self.path == "/responses" else body["messages"]
        prompt = history[-1]["content"]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        if prompt.startswith("NEXT-"):
            self.emit(self.text_event("READY-" + prompt))
            self.emit({"type": "response.completed", "response": {"output": []}} if self.path == "/responses" else {"type": "message_stop"})
        elif self.server.scenario == "tool":
            for event in sse_call(self.path, held_tool(prompt)):
                self.emit(event)
        else:
            self.emit(self.text_event("CANCELWAIT-" + prompt))
            # Read-side EOF proves the production HTTP client closed its request.
            while not self.server.stop.is_set():
                ready, _, _ = select.select([self.connection], [], [], .02)
                if ready and not self.connection.recv(1, socket.MSG_PEEK):
                    self.server.disconnected_at = time.monotonic()
                    return


def identity(pid):
    try:
        stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return stat[19] if stat[0] != "Z" else None
    except (FileNotFoundError, ProcessLookupError):
        return None


def descendants(pid):
    found = {}
    pending = [pid]
    while pending:
        parent = pending.pop()
        for task in Path(f"/proc/{parent}/task").glob("*/children"):
            try:
                children = task.read_text().split()
            except (FileNotFoundError, ProcessLookupError):
                continue
            for child in map(int, children):
                if child not in found:
                    found[child] = identity(child)
                    pending.append(child)
    return found


def case(adapter, server, drop_cancel):
    with tempfile.TemporaryDirectory(prefix="demoncoder-cancel-") as directory:
        workspace = Path(directory)
        subprocess.run(["git", "init", "-q", directory], check=True)
        (workspace / "cancellation").write_text(server.scenario)
        token = uuid.uuid4().hex[:12]
        settings = f'default_connection = "selected"\n[connections.selected]\nadapter = "{adapter}"\nmodel = "fixture-model"\n'
        native = adapter in ("openai-api", "anthropic-api")
        if native:
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
        owned = {}
        heartbeat = workspace / "heartbeat"
        try:
            until(master, process, output, b"Prompt")
            os.write(master, token.encode() + b"\r")
            until(master, process, output, ("CANCELWAIT-" + token).encode(), timeout=3)
            if not native or server.scenario == "tool":
                deadline = time.monotonic() + 3
                while not heartbeat.exists() or heartbeat.stat().st_size < 3:
                    assert time.monotonic() < deadline, "held subprocess never wrote its heartbeat"
                    time.sleep(.01)
                owned = descendants(process.pid)
                assert len(owned) >= 2, "fixture did not start a child process"
            started = time.monotonic()
            if not drop_cancel:
                os.write(master, b"\x1b")
            deadline = started + 2
            while time.monotonic() < deadline:
                time.sleep(min(.02, max(0, deadline - time.monotonic())))
            alive = [pid for pid, birth in owned.items() if birth and identity(pid) == birth]
            assert not alive, f"owned subprocesses still active after two seconds: {alive}"
            if native and server.scenario == "provider":
                assert server.disconnected_at is not None, "HTTP request still open after two seconds"
                assert server.disconnected_at <= deadline, "HTTP abort exceeded two seconds"
            size = heartbeat.stat().st_size if heartbeat.exists() else 0
            time.sleep(.15)
            assert not heartbeat.exists() or heartbeat.stat().st_size == size, "subprocess continued marker activity after grace period"
            events = [json.loads(line) for line in log.read_text().splitlines()]
            assert any(row["event"]["type"] == "turn_finished" and row["event"].get("status") == "cancelled" for row in events), "cancelled outcome missing"
            os.write(master, ("NEXT-" + token).encode() + b"\r")
            until(master, process, output, ("READY-NEXT-" + token).encode(), timeout=3)
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0
        finally:
            server.stop.set()
            owned.update(descendants(process.pid))
            for pid, birth in owned.items():
                if birth and identity(pid) == birth:
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fault-drop-cancel", action="store_true")
    args = parser.parse_args()
    failed = []
    for adapter in ["openai-api", "anthropic-api", "codex", "claude"]:
        for scenario in ["provider", "tool"]:
            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
            server.scenario = scenario
            server.stop = threading.Event()
            server.disconnected_at = None
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                case(adapter, server, args.fault_drop_cancel)
                print("CODE-005", adapter, scenario, "stopped within two seconds; next prompt accepted", flush=True)
            except (AssertionError, OSError, subprocess.SubprocessError) as error:
                failed.append((adapter, scenario))
                print("CODE-005", adapter, scenario, "FAILED:", str(error), flush=True)
            finally:
                server.stop.set()
                server.shutdown()
                server.server_close()
                thread.join(timeout=2)
    print("cairn: CODE-005: " + ("fail" if failed else "pass"))
    return int(bool(failed))


if __name__ == "__main__":
    raise SystemExit(main())
