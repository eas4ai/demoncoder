#!/usr/bin/env python3
"""Exercise host execution and outside-access decisions with harmless canaries."""
import fcntl
import http.server
import itertools
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
import time
import unittest

sys.dont_write_bytecode = True
from terminal_session import BINARY, Provider as CodingProvider, case, until
from tool_cycle_fixture import sse_call


class Provider(CodingProvider):
    def send_events(self, events):
        data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def text(self, text):
        if self.path == "/oracle-anthropic":
            return self.send_events([
                {"type":"message_start", "message":{"usage":{"input_tokens":17}}},
                {"type":"content_block_delta", "index":0, "delta":{"type":"text_delta", "text":text}},
                {"type":"message_delta", "delta":{"stop_reason":"end_turn"}, "usage":{"output_tokens":4}},
                {"type":"message_stop"},
            ])
        self.send_events([
            {"type":"response.output_text.delta", "delta":text},
            {"type":"response.completed", "response":{"output":[], "usage":{"input_tokens":17,"output_tokens":4}}},
        ])

    def do_POST(self):
        if self.path not in ("/oracle", "/oracle-anthropic", "/host"):
            return super().do_POST()
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path.startswith("/oracle"):
            assert body["tools"] == [], "Oracle must advertise no tools"
            prompt = body["messages"][-1]["content"] if self.path == "/oracle-anthropic" else body["input"][-1]["content"]
            if isinstance(prompt, list):
                prompt = prompt[0]["text"]
            request = json.loads(prompt.strip().splitlines()[-1])
            self.server.reviews.append(request)
            mode = self.server.mode
            if mode == "unavailable":
                self.send_response(503)
                self.end_headers()
                return
            if mode == "invalid":
                return self.text("This is not a decision.")
            if mode == "tool":
                return self.send_events(sse_call("/messages" if self.path == "/oracle-anthropic" else "/responses", {"id":"oracle-write", "name":"write", "arguments":{"path":"oracle-effect.txt","content":"must not exist"}}))
            return self.text(json.dumps({"decision":"allow" if mode == "allow" else "deny", "reason":"Controlled outside-access decision."}))
        assert len(body["tools"]) == 4
        assert "No sandbox" in next(tool["description"] for tool in body["tools"] if tool["name"] == "bash")
        results = [item for item in body["input"] if item.get("type") == "function_call_output"]
        if results:
            self.server.results = [json.loads(item["output"]) for item in results]
            return self.text("HOST-CYCLE-DONE")
        self.send_events([{"type":"response.completed", "response":{"output":[
            {"type":"function_call","call_id":call["id"],"name":call["name"],"arguments":json.dumps(call["arguments"])} for call in self.server.calls
        ]}}])


def server():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    server.received = []
    server.tool_cycles = True
    server.wrong_edit = False
    server.cycles = {}
    server.mode = "allow"
    server.reviews = []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


class HostAccess(unittest.TestCase):
    def test_all_connections_execute_host_tools_through_the_oracle(self):
        peer, thread = server()
        try:
            for adapter in ("openai-api", "anthropic-api", "codex", "claude"):
                with self.subTest(adapter=adapter):
                    before = len(peer.reviews)
                    case(adapter, peer, False, True, host_access=True)
                    self.assertEqual(len(peer.reviews), before + 1)
                    self.assertEqual(peer.reviews[-1]["proposed_tool"]["name"], "bash")
        finally:
            peer.shutdown()
            peer.server_close()
            thread.join(timeout=2)

    def test_outside_paths_and_oracle_failures(self):
        for oracle_adapter, mode in itertools.product(("openai-api", "anthropic-api"), ("allow", "deny", "invalid", "unavailable", "tool")):
            with self.subTest(oracle=oracle_adapter, mode=mode), tempfile.TemporaryDirectory(prefix="demoncoder-host-") as directory:
                root = Path(directory)
                workspace = root / "project"
                workspace.mkdir()
                outside = root / "outside.txt"
                outside.write_text("outside-canary")
                (workspace / "link").symlink_to(outside)
                peer, thread = server()
                peer.mode = mode
                peer.results = []
                peer.calls = [
                    {"id":"inside", "name":"write", "arguments":{"path":"inside.txt","content":"inside-ok"}},
                    {"id":"outside-read", "name":"read", "arguments":{"path":str(outside)}},
                    {"id":"outside-link", "name":"read", "arguments":{"path":"link"}},
                    {"id":"outside-write", "name":"write", "arguments":{"path":str(root / "created.txt"),"content":"reviewed-write"}},
                    {"id":"outside-edit", "name":"edit", "arguments":{"path":str(outside),"old_text":"outside-canary","new_text":"reviewed-edit"}},
                    {"id":"host-bash", "name":"bash", "arguments":{"command":"set -e; printf '%s\\n' \"$TMPDIR\"; printf scratch-ok > \"$TMPDIR/scratch.txt\"; test \"$PWD\" != /workspace; test -z \"${OPENAI_API_KEY-}\"; printf host-ok"}},
                ]
                config = root / "settings.toml"
                oracle_path = "oracle-anthropic" if oracle_adapter == "anthropic-api" else "oracle"
                config.write_text(f'default_connection="coding"\n[oracle]\nconnection="reviewer"\n[connections.coding]\nadapter="openai-api"\nmodel="fixture-model"\nendpoint="http://127.0.0.1:{peer.server_port}/host"\n[connections.reviewer]\nadapter="{oracle_adapter}"\nmodel="oracle-model"\nendpoint="http://127.0.0.1:{peer.server_port}/{oracle_path}"\n')
                log = root / "events.jsonl"
                master, slave = pty.openpty()
                fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
                process = subprocess.Popen([str(BINARY), "--yolo", "--workspace", str(workspace), "--config", str(config), "--event-log", str(log)], stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
                    env={"PATH":"/usr/bin:/bin","HOME":str(root),"TERM":"xterm-256color","LANG":"C.UTF-8","OPENAI_API_KEY":"synthetic-host-key","ANTHROPIC_API_KEY":"synthetic-host-key"})
                os.close(slave)
                output = bytearray()
                try:
                    until(master, process, output, b"Prompt")
                    self.assertIn(b"HOST ACCESS", output)
                    os.write(master, b"Exercise harmless fixture files and scratch space.\r")
                    until(master, process, output, b"HOST-CYCLE-DONE", timeout=12)
                    results = {result["call_id"]: result for result in peer.results}
                    self.assertTrue(results["inside"]["success"])
                    self.assertEqual((workspace / "inside.txt").read_text(), "inside-ok")
                    self.assertEqual(len(peer.reviews), 5)
                    self.assertEqual(peer.reviews[1]["resolved_target"], str(outside))
                    for name in ("outside-read", "outside-link", "outside-write", "outside-edit", "host-bash"):
                        self.assertEqual(results[name]["success"], mode == "allow", name)
                    self.assertFalse((workspace / "oracle-effect.txt").exists())
                    if mode == "allow":
                        self.assertEqual((root / "created.txt").read_text(), "reviewed-write")
                        self.assertEqual(outside.read_text(), "reviewed-edit")
                        scratch = Path(results["host-bash"]["output"].splitlines()[0])
                        self.assertEqual(scratch.parent, Path("/tmp"))
                        self.assertEqual(scratch.stat().st_mode & 0o777, 0o700)
                        self.assertEqual((scratch / "scratch.txt").read_text(), "scratch-ok")
                        (scratch / "scratch.txt").unlink()
                        scratch.rmdir()
                    else:
                        self.assertFalse((root / "created.txt").exists())
                        self.assertEqual(outside.read_text(), "outside-canary")
                    rows = [json.loads(line)["event"] for line in log.read_text().splitlines()]
                    decisions = [row for row in rows if row["type"] == "tool_review" and row["decision"] != "reviewing"]
                    self.assertEqual(len(decisions), 5)
                    self.assertNotIn("synthetic-host-key", log.read_text())
                    os.write(master, b"\x11")
                    process.wait(timeout=5)
                    self.assertEqual(process.returncode, 0)
                except AssertionError as error:
                    recent = [json.loads(line)["event"] for line in log.read_text().splitlines()][-6:] if log.exists() else []
                    raise AssertionError(f"{error}; recent events: {recent}; result count: {len(peer.results)}") from error
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait(timeout=5)
                    os.close(master)
                    peer.shutdown()
                    peer.server_close()
                    thread.join(timeout=2)


if __name__ == "__main__":
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(HostAccess))
    raise SystemExit(not result.wasSuccessful())
