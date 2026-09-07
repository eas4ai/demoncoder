#!/usr/bin/env python3
"""Exercise installed backend tool routing against a local model peer."""
import base64
import argparse
import datetime
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
import uuid
from urllib.parse import urlparse

sys.dont_write_bytecode = True
from provider_metadata import ModelMetadataHandler
from terminal_session import BINARY, ROOT, until
from boundary_fixture import requests, model_results, check_result
import result_fixture


def fake_codex_auth(home):
    def encode(value):
        return base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip("=")
    token = encode({"alg": "none", "typ": "JWT"}) + "." + encode({"email": "fixture@example.invalid", "https://api.openai.com/auth": {"chatgpt_plan_type": "plus", "chatgpt_account_id": "fixture-account"}}) + ".c2lnbmF0dXJl"
    (home / "auth.json").write_text(json.dumps({"auth_mode": "chatgpt", "tokens": {"id_token": token, "access_token": "synthetic-access", "refresh_token": "synthetic-refresh", "account_id": "fixture-account"}, "last_refresh": datetime.datetime.now(datetime.timezone.utc).isoformat()}))


class Model(ModelMetadataHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        if "/models/" in urlparse(self.path).path:
            return super().do_GET()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"models":[],"items":[]}')

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        try:
            body = json.loads(body)
        except (ValueError, UnicodeDecodeError):
            self.server.errors.append("unparsed request: " + str(self.headers.get("Content-Encoding")))
            self.send_response(400)
            self.end_headers()
            return
        if not urlparse(self.path).path.endswith(("/responses", "/messages")):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"input_tokens":1}')
            return
        self.server.requests.append(body)
        self.server.paths.append(self.path)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        openai = self.server.adapter in ["codex", "openai-api"]
        step = self.server.step
        try:
            if step:
                previous = self.server.calls[step - 1]
                result = model_results(body, openai)[previous["id"]]
                if self.server.result_cycle and self.server.fault_result and previous["id"] == "result-1":
                    # A fixture-only lie about the actual failing check. The
                    # ordinary assertions must reject it; tool code is intact.
                    forged = json.loads(result)
                    forged.update(success=True, exit_code=0)
                    result = json.dumps(forged)
                checker = result_fixture.check_result if self.server.result_cycle else check_result
                checker(previous, result)
                self.server.results.append(result)
                if self.server.result_cycle and previous["id"] == "result-1":
                    self.server.failure_ready.set()
                    assert self.server.failure_rendered.wait(5), "failed verification was not rendered before correction"
        except (AssertionError, ValueError, KeyError, TypeError) as error:
            self.server.errors.append(f"step {step}: {error}")
            self.wfile.write(b'event: error\ndata: {"type":"error"}\n\n')
            self.wfile.flush()
            return
        call = self.server.calls[step] if step < len(self.server.calls) else None
        self.server.step += 1
        item = {"type": "message", "id": "msg_fixture", "role": "assistant", "content": [{"type": "output_text", "text": "INSTALLED-READY"}]}
        if openai:
            if call:
                item = {"type": "custom_tool_call", "id": "ct_" + call["id"], "call_id": call["id"], "name": call["name"], "input": call["input"]} if "input" in call else {"type": "function_call", "id": "fc_" + call["id"], "call_id": call["id"], "name": call["name"], "arguments": json.dumps(call["arguments"])}
            events = [
                {"type": "response.created", "response": {"id": "resp_fixture", "status": "in_progress", "output": []}},
                {"type": "response.output_item.added", "output_index": 0, "item": {**item, "content": []}},
                {"type": "response.output_text.delta", "item_id": "msg_fixture", "output_index": 0, "content_index": 0, "delta": "INSTALLED-READY"},
                {"type": "response.output_item.done", "output_index": 0, "item": item},
                {"type": "response.completed", "response": {"id": "resp_fixture", "status": "completed", "output": [item], "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}}},
            ]
            if call:
                events = [event for event in events if event["type"] != "response.output_text.delta"]
                events[1]["item"] = item
        else:
            events = [
                {"type": "message_start", "message": {"id": "msg_fixture", "type": "message", "role": "assistant", "model": "claude-sonnet-4-6", "content": [], "stop_reason": None, "usage": {"input_tokens": 10, "output_tokens": 0}}},
                {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
                {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "INSTALLED-READY"}},
                {"type": "content_block_stop", "index": 0},
                {"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": None}, "usage": {"output_tokens": 5}},
                {"type": "message_stop"},
            ]
            if call:
                events[1]["content_block"] = {"type": "tool_use", "id": call["id"], "name": call["name"], "input": {}}
                events[2]["delta"] = {"type": "input_json_delta", "partial_json": json.dumps(call["arguments"])}
                events[4]["delta"]["stop_reason"] = "tool_use"
        for event in events:
            self.wfile.write(("event: " + event["type"] + "\ndata: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()


def case(adapter, server):
    native = adapter in ["openai-api", "anthropic-api"]
    binary = None if native else shutil.which(adapter)
    assert native or binary, f"prerequisite missing: installed {adapter}"
    binary = str(Path(binary).resolve()) if binary else None
    with tempfile.TemporaryDirectory(prefix="demoncoder-installed-") as directory:
        parent = Path(directory)
        workspace = parent / "workspace"
        workspace.mkdir()
        home = parent / "home"
        home.mkdir()
        codex_home = home / ".codex"
        codex_home.mkdir()
        subprocess.run(["git", "init", "-q", str(workspace)], check=True)
        (parent / "outside").mkdir()
        secret = "canary-" + uuid.uuid4().hex
        (parent / "outside/canary.txt").write_text("OUTSIDE-DOCUMENTATION")
        (home / ".demoncoder").mkdir()
        (home / ".demoncoder/credential-canary").write_text(secret)
        (codex_home / "BEST_PRACTICES.md").write_text("MACHINE-STANDARDS")
        (workspace / ".demoncoder").mkdir()
        (workspace / ".demoncoder/protected").write_text(secret)
        (workspace / ".git/protected").write_text(secret)
        server.token = uuid.uuid4().hex
        server.calls = result_fixture.requests(adapter, server.token) if server.result_cycle else requests(adapter, parent, secret)
        endpoint = f"http://127.0.0.1:{server.server_port}"
        if adapter == "codex":
            fake_codex_auth(codex_home)
            marker_script = f"from pathlib import Path; Path({str(parent / 'inherited-mcp-started')!r}).write_text('unauthorized startup')"
            (codex_home / "config.toml").write_text(f'''model = "gpt-5.4"
model_provider = "fixture"
chatgpt_base_url = "{endpoint}"
cli_auth_credentials_store = "file"
[features]
enable_request_compression = false
[model_providers.fixture]
name = "Fixture"
base_url = "{endpoint}/v1"
wire_api = "responses"
requires_openai_auth = true
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
[mcp_servers.inherited]
command = "/usr/bin/python3"
args = ["-c", {json.dumps(marker_script)}]
startup_timeout_sec = 1
''')
        (workspace / "installed-backend.json").write_text(json.dumps({"adapter": adapter, "binary": binary, "endpoint": endpoint}))
        config = workspace / "connection.toml"
        model = "gpt-5.4" if adapter == "codex" else "claude-sonnet-4-6"
        settings = f'default_connection = "selected"\n[connections.selected]\nadapter = "{adapter}"\nmodel = "{model}"\n'
        if native:
            route = "responses" if adapter == "openai-api" else "messages"
            settings += f'endpoint = "{endpoint}/v1/{route}"\n'
        else:
            settings += f'binary = {json.dumps(str(ROOT / "tests/installed_backend_launcher.py"))}\n'
        config.write_text(settings)
        log = parent / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 160, 0, 0))
        env = {"PATH": "/usr/bin:/bin", "HOME": str(home), "CODEX_HOME": str(codex_home), "TERM": "xterm-256color", "LANG": "C.UTF-8", "CLAUDE_CODE_OAUTH_TOKEN": "synthetic-oauth", "OPENAI_API_KEY": "synthetic-key", "ANTHROPIC_API_KEY": "synthetic-key"}
        process = subprocess.Popen([str(BINARY), "--trust-workspace", "--workspace", str(workspace), "--config", str(config), "--event-log", str(log)], stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        try:
            until(master, process, output, b"Prompt")
            os.write(master, b"Check tool availability.\r")
            deadline = time.monotonic() + 20
            while True:
                assert process.poll() is None, "application exited before completion"
                if select.select([master], [], [], .01)[0]:
                    output.extend(os.read(master, 65536))
                if server.result_cycle and server.failure_ready.is_set() and ("VERIFY-FAILED-" + server.token).encode() in output:
                    server.failure_rendered.set()
                records = [json.loads(line)["event"] for line in log.read_text().splitlines(keepends=True) if line.endswith("\n")]
                finished = [row for row in records if row["type"] == "turn_finished"]
                if finished:
                    assert finished[-1]["status"] == "complete", finished[-1]
                    assert any(row.get("text") == "INSTALLED-READY" for row in records)
                    break
                assert time.monotonic() < deadline, "installed backend did not finish"
            assert server.requests, "installed backend never reached local model"
            expected = {"mcp__demoncoder__" + name for name in ["read", "write", "edit", "bash"]} if adapter == "claude" else {"read", "write", "edit", "bash"}
            for request in server.requests:
                names = {tool.get("name") for tool in request.get("tools", [])}
                assert expected == names, f"backend tool catalog differs from host tools: {names}"
            assert not (parent / "inherited-mcp-started").exists(), "inherited MCP server started outside host admission"
            assert not server.errors and len(server.results) == len(server.calls), server.errors
            if server.result_cycle:
                result_fixture.validate(workspace, server, records, output)
            else:
                assert (workspace / "allowed.txt").read_text() == "second"
            assert (parent / "outside/canary.txt").read_text() == "OUTSIDE-DOCUMENTATION"
            assert (workspace / ".demoncoder/protected").read_text() == secret
            assert (workspace / ".git/protected").read_text() == secret
            assert not (parent / "outside/bypass.txt").exists()
            assert all(secret not in result for result in server.results), "a tool exposed a private credential canary"
            description = "failed check, correction, passing check, and original result identities passed" if server.result_cycle else "ordinary reads, protected writes and credentials, Bash, and backend routing passed"
            print("CODE-008" if server.result_cycle else "CODE-007", adapter, description, flush=True)
        except AssertionError as error:
            events = [json.loads(line)["event"] for line in log.read_text().splitlines()] if log.exists() else []
            print(adapter, "diagnostic", str(error), events[-5:], server.errors, flush=True)
            wire = workspace / "installed-wire.jsonl"
            if wire.exists():
                rows = [json.loads(line) for line in wire.read_text().splitlines()]
                print("wire", rows[-4:], flush=True)
            stderr = workspace / "installed-stderr.txt"
            if stderr.exists():
                print("stderr", stderr.read_text()[-2500:], flush=True)
            raise
        finally:
            os.write(master, b"\x11")
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            os.close(master)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", action="store_true")
    parser.add_argument("--fault-rewrite-result", action="store_true")
    args = parser.parse_args()
    if args.fault_rewrite_result and not args.results:
        parser.error("--fault-rewrite-result requires --results")
    failed = []
    for adapter in ["openai-api", "anthropic-api", "codex", "claude"]:
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
        server.adapter = adapter
        server.requests = []
        server.paths = []
        server.errors = []
        server.step = 0
        server.results = []
        server.result_cycle = args.results
        server.fault_result = args.fault_rewrite_result
        server.failure_ready = threading.Event()
        server.failure_rendered = threading.Event()
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            case(adapter, server)
        except (AssertionError, OSError, subprocess.SubprocessError) as error:
            failed.append(adapter)
            print(adapter, "FAILED", str(error), flush=True)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)
    requirement = "CODE-008" if args.results else "CODE-007"
    print("cairn: " + requirement + ": " + ("fail" if failed else "pass"), flush=True)
    return int(bool(failed))


if __name__ == "__main__":
    raise SystemExit(main())
