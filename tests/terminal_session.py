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
import shlex
import struct
import subprocess
import tempfile
import termios
import threading
import time
import uuid
import sys
sys.dont_write_bytecode = True
from tool_cycle_fixture import Cycle, sse_call
from terminal_screen import screen_text

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/demoncoder"
FIXTURE = ROOT / "tests/backend_fixture.py"


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path == "/responses":
            assert body["store"] is False and "reasoning.encrypted_content" in body["include"]
        if getattr(self.server, "settings_test", None):
            assert body["model"] == self.server.expected_model
            control = body["reasoning"] if self.path == "/responses" else body["output_config"]
            assert control["effort"] == self.server.expected_effort
        call_events = None
        if self.server.tool_cycles:
            assert len(body["tools"]) == 4
            history = body["input"] if self.path == "/responses" else body["messages"]
            prompt = history[0]["content"]
            cycle = self.server.cycles.setdefault(prompt, Cycle(prompt, self.server.wrong_edit))
            result = None
            if len(history) > 1:
                if self.path == "/responses":
                    assert any(item.get("encrypted_content") == "synthetic-reasoning" for item in history)
                result = json.loads(history[-1]["output"] if self.path == "/responses" else history[-1]["content"][0]["content"])
            call = cycle.next(result)
            if call and call["name"] == "bash" and getattr(self.server, "host_workspace", None):
                call["arguments"]["command"] = 'set -e; test "$PWD" = ' + shlex.quote(self.server.host_workspace) + "; " + call["arguments"]["command"]
            if call:
                call_events = sse_call(self.path, call)
                if self.path == "/responses":
                    call_events[0]["response"]["output"].insert(0, {"id":"rs_" + call["id"], "type":"reasoning", "summary":[], "encrypted_content":"synthetic-reasoning"})
            body = {**body, "input": [{"content": prompt}], "messages": [{"content": prompt}]}
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
        data = "".join("data: " + json.dumps(event) + "\n\n" for event in (call_events or events)).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        self.wfile.flush()


def until(master, process, output, needle, timeout=8):
    deadline = time.monotonic() + timeout
    inspected = -1
    while True:
        if needle in output:
            return
        if len(output) != inspected:
            inspected = len(output)
            rows, columns, _, _ = struct.unpack("HHHH", fcntl.ioctl(master, termios.TIOCGWINSZ, b"\0" * 8))
            if needle.decode() in screen_text(output, max(1, columns), max(1, rows)):
                return
        if process.poll() is not None:
            raise AssertionError("application exited before terminal checkpoint: " + needle.decode())
        if time.monotonic() >= deadline:
            raise AssertionError("terminal checkpoint timed out: " + needle.decode())
        if select.select([master], [], [], 0.05)[0]:
            try:
                output.extend(os.read(master, 65536))
            except OSError as error:
                raise AssertionError("PTY closed before checkpoint") from error


def case(adapter, server, fault, tool_cycles=False, host_access=False):
    with tempfile.TemporaryDirectory(prefix="demoncoder-terminal-") as directory:
        workspace = Path(directory)
        server.host_workspace = str(workspace) if host_access else None
        subprocess.run(["git", "init", "-q", str(workspace)], check=True)
        token = "prompt" + uuid.uuid4().hex[:12]
        seed = int(uuid.uuid4().hex[:6], 16)
        if tool_cycles:
            (workspace / "tool-cycle").touch()
            if server.wrong_edit:
                (workspace / "wrong-edit").touch()
            (workspace / "seed.txt").write_text(str(seed) + "\n")
        config = workspace / "connection.toml"
        settings = f'onboarding_complete=true\ndefault_connection = "selected"\n[connections.selected]\nadapter = "{adapter}"\nmodel = "fixture-model"\n'
        if adapter in ("openai-api", "anthropic-api"):
            route = "responses" if adapter == "openai-api" else "messages"
            settings += f'endpoint = "http://127.0.0.1:{server.server_port}/{route}"\n'
        else:
            settings += f'binary = {json.dumps(str(FIXTURE))}\n'
        settings_test = getattr(server, "settings_test", None)
        extra_args = []
        if settings_test:
            server.expected_model = "override-model" if settings_test == "override" else "fixture-model"
            server.expected_effort = "high" if settings_test == "override" else "low"
            settings += 'effort = "low"\n'
            if adapter.endswith("-api"):
                key = "synthetic-" + adapter.removesuffix("-api") + "-key"
                settings += f'api_key = {json.dumps("unused-saved-key" if settings_test == "override" else key)}\n'
            (workspace / "settings-expect.json").write_text(json.dumps({"model":server.expected_model, "effort":server.expected_effort}))
            home_settings = workspace / ".demoncoder/settings.toml"
            home_settings.parent.mkdir()
            home_settings.write_text(settings)
            home_settings.chmod(0o600)
            if settings_test == "override":
                extra_args = ["--model", "override-model", "--effort", "high"]
        if host_access:
            (workspace / "host-access").touch()
            settings = settings.replace("[connections.selected]", '[oracle]\nconnection="reviewer"\n[connections.selected]', 1)
            settings += f'\n[connections.reviewer]\nadapter="openai-api"\nmodel="oracle-model"\nendpoint="http://127.0.0.1:{server.server_port}/oracle"\n'
            extra_args.append("--yolo")
        config.write_text(settings)
        config.chmod(0o600)
        if fault and adapter == "codex":
            (workspace / "drop-prompt").touch()
        log = workspace / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 160, 0, 0))
        env = {"PATH": "/usr/bin:/bin", "HOME": str(workspace), "TERM": "xterm-256color", "LANG": "C.UTF-8",
               "OPENAI_API_KEY": "synthetic-openai-key", "ANTHROPIC_API_KEY": "synthetic-anthropic-key"}
        if settings_test:
            env["CODEX_HOME"] = str(workspace / ".codex-selected")
            env["CLAUDE_CONFIG_DIR"] = str(workspace / ".claude-selected")
        if settings_test == "home":
            env.pop("OPENAI_API_KEY")
            env.pop("ANTHROPIC_API_KEY")
        config_args = [] if settings_test else ["--config", str(config)]
        process = subprocess.Popen([str(BINARY), "--trust-workspace", "--workspace", str(workspace), *config_args, *extra_args, "--event-log", str(log)],
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
            if tool_cycles:
                results = [record["event"]["result"] for record in records if record["event"]["type"] == "tool_finished"]
                assert [r["tool"] for r in results] == ["read", "write", "edit", "bash"], "missing production tool execution"
                assert all(r["success"] for r in results), "tool cycle reported a failure"
                assert len(set(r["call_id"] for r in results)) == 4, "tool call identities were reused"
                assert results[0]["output"].strip() == str(seed), "read did not obtain actual seed"
                assert results[-1]["exit_code"] == 0 and "VERIFIED-" + token in results[-1]["output"]
                assert (workspace / "answer.py").read_text() == f"value = {seed + 1}\n", "actual repository change differs"
            if host_access:
                assert any(record["event"]["type"] == "tool_review" and record["event"]["decision"] == "allowed" for record in records)
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
    parser.add_argument("--tools", action="store_true")
    parser.add_argument("--settings", choices=["home", "override"])
    parser.add_argument("--fault-wrong-edit", action="store_true")
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    server.received = []
    server.settings_test = args.settings
    server.tool_cycles = args.tools
    server.wrong_edit = args.fault_wrong_edit
    server.cycles = {}
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    failed = []
    try:
        for adapter in ["openai-api", "anthropic-api", "codex", "claude"]:
            try:
                case(adapter, server, args.fault_drop_prompt, args.tools)
                print("CODE-002" if args.tools else "CODE-001", adapter, "production tool cycle passed" if args.tools else "terminal prompt reached runtime and response was rendered", flush=True)
            except (AssertionError, OSError, subprocess.SubprocessError) as error:
                failed.append(adapter)
                print("CODE-002" if args.tools else "CODE-001", adapter, "FAILED:", str(error), flush=True)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
    print("cairn: " + ("CODE-002" if args.tools else "CODE-001") + ": " + ("fail" if failed else "pass"))
    return int(bool(failed))


if __name__ == "__main__":
    raise SystemExit(main())
