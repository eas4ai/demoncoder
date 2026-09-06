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
from steering_fixture import initial_calls, corrected_call


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def emit(self, event):
        self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()

    def calls(self, calls):
        if self.path == "/responses":
            self.emit({"type":"response.completed", "response":{"output":[{
                "type":"function_call", "call_id":call["id"], "name":call["name"], "arguments":json.dumps(call["arguments"]),
            } for call in calls]}})
        else:
            self.emit({"type":"message_start", "message":{"usage":{"input_tokens":9}}})
            for index, call in enumerate(calls):
                for event in sse_call(self.path, call):
                    if event["type"].startswith("content_block_"):
                        event["index"] = index
                        self.emit(event)
            self.emit({"type":"message_stop"})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        history = body["input"] if self.path == "/responses" else body["messages"]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.server.requests.append(body)
        if len(self.server.requests) == 1:
            self.calls(initial_calls(history[0]["content"]))
        elif len(self.server.requests) == 2:
            correction = history[-1]["content"]
            assert correction.startswith("CORRECT-")
            if self.path == "/responses":
                results = [json.loads(item["output"]) for item in history if item.get("type") == "function_call_output"]
            else:
                results = [json.loads(block["content"]) for item in history if isinstance(item["content"], list) for block in item["content"] if block["type"] == "tool_result"]
            assert len(results) == 2 and results[0]["call_id"] == "held" and results[0]["success"]
            assert "HELD-DONE" in results[0]["output"] and results[0]["exit_code"] == 0
            assert results[1]["call_id"] == "superseded" and not results[1]["success"]
            self.calls([corrected_call(correction)])
        else:
            assert len(self.server.requests) == 3
            if self.path == "/responses":
                self.emit({"type":"response.output_text.delta", "delta":"STEER-APPLIED"})
                self.emit({"type":"response.completed", "response":{"output":[]}})
            else:
                self.emit({"type":"content_block_delta", "index":0, "delta":{"type":"text_delta", "text":"STEER-APPLIED"}})
                self.emit({"type":"message_stop"})


def case(adapter, server, drop_correction):
    with tempfile.TemporaryDirectory(prefix="demoncoder-steering-") as directory:
        workspace = Path(directory)
        subprocess.run(["git", "init", "-q", directory], check=True)
        (workspace / "steering").touch()
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
        process = subprocess.Popen([str(BINARY), "--workspace", directory, "--config", str(config), "--event-log", str(log)], stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        correction = "CORRECT-" + token
        try:
            until(master, process, output, b"Prompt")
            os.write(master, token.encode() + b"\r")
            until(master, process, output, ("STEER-WAIT-" + token).encode(), timeout=3)
            os.write(master, correction.encode() + (b"" if drop_correction else b"\r"))
            until(master, process, output, b"[Correction", timeout=3)
            records = [json.loads(line)["event"] for line in log.read_text().splitlines()]
            assert any(e["type"] == "text" and "Correction queued for the next tool boundary" in e["text"] for e in records)
            assert not any(e["type"] in ("tool_finished", "turn_finished") for e in records)
            (workspace / "release-tool").touch()
            until(master, process, output, b"STEER-APPLIED", timeout=5)
            assert not (workspace / "superseded.txt").exists(), "superseded tool left its marker"
            assert (workspace / "corrected.txt").read_text() == correction
            records = [json.loads(line)["event"] for line in log.read_text().splitlines()]
            calls = [e["call"] for e in records if e["type"] == "tool_started"]
            results = [e["result"] for e in records if e["type"] == "tool_finished"]
            assert [c["name"] for c in calls] == ["bash", "write"]
            assert len(results) == 2 and all(r["success"] for r in results)
            assert results[0]["call_id"] == calls[0]["id"] and "HELD-DONE" in results[0]["output"]
            if adapter in ("codex", "claude"):
                audit = json.loads((workspace / "steering-audit.json").read_text())
                assert audit["correction"] == correction and audit["completed_result"] == results[0]
                assert audit["denied"] == ["late", "superseded"]
            else:
                assert len(server.requests) == 3
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0
        finally:
            (workspace / "release-tool").touch()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fault-drop-correction", action="store_true")
    args = parser.parse_args()
    failed = []
    for adapter in ["openai-api", "anthropic-api", "codex", "claude"]:
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
        server.requests = []
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            case(adapter, server, args.fault_drop_correction)
            print("CODE-004", adapter, "correction acknowledged, old tools denied, corrected model input and effect passed", flush=True)
        except (AssertionError, OSError, subprocess.SubprocessError) as error:
            failed.append(adapter)
            print("CODE-004", adapter, "FAILED:", str(error), flush=True)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)
    print("cairn: CODE-004: " + ("fail" if failed else "pass"))
    return int(bool(failed))


if __name__ == "__main__":
    raise SystemExit(main())
