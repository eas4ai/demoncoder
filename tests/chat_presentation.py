#!/usr/bin/env python3
"""Inspect compact/full chat through the production terminal and native tools."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
from scrollback import App
from provider_metadata import ModelMetadataHandler


class Provider(ModelMetadataHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests = getattr(self.server, "requests", []) + [body]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        prompt = body["input"][-1]
        if prompt.get("type") == "function_call_output":
            text = "REVIEW-DONE"
            output = []
        elif prompt["content"] == "read-code":
            text = "Inspecting the source."
            output = [{"type": "function_call", "call_id": "source-read", "name": "read",
                       "arguments": json.dumps({"path": "example.rs"})}]
        elif prompt["content"] in ("run-tool", "fail-tool", "cancel-tool"):
            command = {"run-tool": "printf 'STREAM-MARKER\\n'; sleep 0.3; printf 'FINAL-MARKER\\n'",
                       "fail-tool": "printf 'FAIL-MARKER\\n'; exit 7",
                       "cancel-tool": "printf 'HELD-MARKER\\n'; sleep 30"}[prompt["content"]]
            text = "Executing the command."
            output = [{"type": "function_call", "call_id": "command", "name": "bash",
                       "arguments": json.dumps({"command": command})}]
        else:
            text = "HEAD-MARKER\n" + "".join(f"OUTPUT-{n:03d} " + "x" * 130 + "\n" for n in range(40)) + "TAIL-MARKER"
            output = []
        for event in [{"type": "response.output_text.delta", "delta": text},
                      {"type": "response.completed", "response": {"output": output, "usage": {"output_tokens": 100}}}]:
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()


class ChatPresentation(unittest.TestCase):
    def test_long_wrapped_output_expands_without_resubmitting_or_losing_input(self):
        with tempfile.TemporaryDirectory() as directory:
            app = App(Path(directory), Provider)
            try:
                app.send(b"long-output\r")
                compact = app.wait(lambda s: "TAIL-MARKER" in s and "Ctrl-O" in s and "hidden" in s and "· complete ·" in s, "completed compact wrapped output")
                self.assertIn("● Assistant", compact)
                self.assertIn("HEAD-MARKER", compact)
                self.assertNotIn("OUTPUT-020", compact)
                self.assertTrue(all(line[-2:] == "  " for line in compact.splitlines()), "right gutter is not empty")
                app.send(b"unsent correction\x0f\x1b[H")
                app.wait(lambda s: "HEAD-MARKER" in s and "Full output" in s, "expanded transcript")
                for _ in range(5):
                    if "OUTPUT-020" in app.screen():
                        break
                    previous = app.screen().splitlines()[1:-5]
                    app.send(b"\x1b[6~")
                    app.wait(lambda s: s.splitlines()[1:-5] != previous, "next expanded page")
                self.assertIn("OUTPUT-020", app.screen())
                self.assertIn("unsent correction", app.screen())
                self.assertEqual(len(app.server.requests), 1)
                app.send(b"\x0f\x1b[F")
                app.wait(lambda s: "hidden" in s and "TAIL-MARKER" in s, "restored compact transcript")
                app.resize(4, 8)
                app.send(b"\x0f")
                app.resize(35, 100)
                app.wait(lambda s: "TAIL-MARKER" in s, "restored expanded terminal")
            finally:
                app.close()

    def test_streaming_success_failure_and_interruption_are_distinct(self):
        for prompt, marker, outcome in [("run-tool", "STREAM-MARKER", "● Ran"),
                                         ("fail-tool", "FAIL-MARKER", "● Failed bash"),
                                         ("cancel-tool", "HELD-MARKER", "● Stopped bash")]:
            with self.subTest(prompt=prompt), tempfile.TemporaryDirectory() as directory:
                app = App(Path(directory), Provider)
                try:
                    app.send(prompt.encode() + b"\r")
                    if prompt != "fail-tool":
                        running = app.wait(lambda s: marker in [line.strip() for line in s.splitlines()] and "● Running bash" in s, "running tool output")
                        self.assertNotIn("● Ran", running)
                    if prompt == "cancel-tool":
                        app.send(b"\x1b")
                    screen = app.wait(lambda s: outcome in s and ("cancelled" in s if prompt == "cancel-tool" else "REVIEW-DONE" in s), "truthful tool outcome")
                    # The command heading also quotes this marker; inspect body rows.
                    body = [line.strip() for line in screen.splitlines()]
                    self.assertEqual(body.count(marker), 1, app.screen())
                    if prompt == "fail-tool":
                        self.assertIn("exit 7", screen)
                    if prompt == "run-tool":
                        self.assertEqual(body.count("FINAL-MARKER"), 1)
                finally:
                    app.close()

    def test_source_read_has_operation_target_and_syntax_colors(self):
        with tempfile.TemporaryDirectory() as directory:
            def prepare(workspace, _home):
                (workspace / "example.rs").write_text('fn main() {\n    let greeting = "hello";\n    println!("{greeting}");\n}\n')
            app = App(Path(directory), Provider, prepare=prepare)
            try:
                app.send(b"read-code\r")
                screen = app.wait(lambda s: "REVIEW-DONE" in s, "completed source read")
                self.assertIn("● Read example.rs", screen)
                self.assertIn("let greeting", screen)
                self.assertEqual(screen.count("let greeting"), 1)
                self.assertIn(b"38;2;", app.output, "source read has no RGB syntax styles")
            finally:
                app.close()


if __name__ == "__main__":
    unittest.main()
