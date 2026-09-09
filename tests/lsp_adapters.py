#!/usr/bin/env python3
"""LSP through all four production adapters, using controlled model peers."""
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

sys.dont_write_bytecode = True
from terminal_session import BINARY, FIXTURE, ROOT, until
from provider_metadata import ModelMetadataHandler
from lsp_cycle_fixture import Cycle
from tool_cycle_fixture import sse_call


class Provider(ModelMetadataHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        try:
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            assert {tool["name"] for tool in body["tools"]} == {"read", "write", "edit", "bash", "lsp"}
            openai = self.path == "/responses"
            history = body["input" if openai else "messages"]
            token = history[0]["content"]
            cycle = self.server.cycles.setdefault(token, Cycle(token))
            result = None
            if cycle.step:
                result = json.loads(history[-1]["output"] if openai else history[-1]["content"][0]["content"])
            call = cycle.next(result)
            if call:
                events = sse_call(self.path, call)
            else:
                text = "RECEIVED-" + token
                events = ([{"type": "response.output_text.delta", "delta": text},
                           {"type": "response.completed", "response": {"output": []}}] if openai else
                          [{"type": "content_block_delta", "delta": {"type": "text_delta", "text": text}},
                           {"type": "message_stop"}])
            data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except Exception as error:
            self.server.errors.append(repr(error))
            self.send_error(500)


def case(adapter, server):
    scratch = Path("/home/shawn/workspace2/scratchpads")
    scratch.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="demoncoder-lsp-adapter-", dir=scratch) as directory:
        home = Path(directory)
        workspace = home / "project"
        workspace.mkdir()
        subprocess.run(["git", "init", "-q", str(workspace)], check=True)
        (workspace / "lsp-cycle").touch()
        (workspace / ".fixture-mode").write_text("normal")
        language_server = home / "language-server"
        language_server.write_bytes((ROOT / "tests/lsp_fixture.py").read_bytes())
        language_server.chmod(0o700)
        source = "// π😀\nfn target() { /* BROKEN */ }\nfn main() { target(); }\n"
        (workspace / "main.rs").write_text(source)
        config = home / "connection.toml"
        settings = f'onboarding_complete=true\ndefault_connection="selected"\n[connections.selected]\nadapter="{adapter}"\nmodel="fixture-model"\n'
        if adapter.endswith("-api"):
            route = "responses" if adapter == "openai-api" else "messages"
            settings += f'endpoint="http://127.0.0.1:{server.server_port}/{route}"\n'
        else:
            settings += f'binary={json.dumps(str(FIXTURE))}\n'
        config.write_text(settings)
        config.chmod(0o600)
        log = home / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 160, 0, 0))
        env = {"PATH": "/usr/bin:/bin", "HOME": str(home), "TERM": "xterm-256color", "LANG": "C.UTF-8",
               "OPENAI_API_KEY": "synthetic-openai-key", "ANTHROPIC_API_KEY": "synthetic-anthropic-key"}
        process = subprocess.Popen([str(BINARY), "--trust-workspace", "--workspace", str(workspace),
            "--config", str(config), "--rust-language-server", str(language_server),
            "--event-log", str(log)], stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        token = "lsp-" + adapter
        try:
            until(master, process, output, b"Prompt")
            os.write(master, token.encode() + b"\r")
            try:
                until(master, process, output, ("RECEIVED-" + token).encode(), timeout=30)
            except AssertionError as error:
                recorded = log.read_text()[-16000:] if log.exists() else "no event log"
                raise AssertionError(f"{error}; provider errors={server.errors}; events={recorded}") from error
            assert not server.errors, server.errors
            rows = [json.loads(line) for line in log.read_text().splitlines()]
            results = [row["event"]["result"] for row in rows if row["event"]["type"] == "tool_finished"]
            assert len(results) == 17, results
            assert [r["success"] for r in results] == [index not in (8, 11, 13, 15) for index in range(1, 18)]
            assert len({r["call_id"] for r in results}) == 17
            assert (workspace / "main.rs").read_text() == source.replace("BROKEN", "FIXED")
            assert not any(row["event"]["type"] == "verification_finished" for row in rows)
            assert "synthetic-openai-key" not in log.read_text()
            assert "synthetic-anthropic-key" not in log.read_text()
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0
            print(f"{adapter}: LSP status/navigation/diagnostics, edits, empty results, unsupported methods, oversized frames and invalid Unicode passed")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)


def main():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    server.cycles, server.errors = {}, []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for adapter in ("openai-api", "anthropic-api", "codex", "claude"):
            case(adapter, server)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


if __name__ == "__main__":
    main()
