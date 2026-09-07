#!/usr/bin/env python3
"""Output settings and truncation through the installed application interface."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
sys.dont_write_bytecode = True
from scrollback import App
from provider_metadata import ModelMetadataHandler

PAYLOAD = "word " * 10000

class Provider(ModelMetadataHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests = getattr(self.server, "requests", []) + [body]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        if self.path == "/responses":
            events = [{"type":"response.output_text.delta", "delta":"LIMIT-CHECK-DONE"},
                      {"type":"response.completed", "response":{"output":[], "usage":{"output_tokens":12000}}}]
        else:
            history = body["messages"]
            last = history[-1]["content"]
            stop = "end_turn"
            events = [{"type":"message_start", "message":{"usage":{"input_tokens":17}}}]
            if last == "large-file":
                enough = body["max_tokens"] >= 12000
                stop = "tool_use" if enough else "max_tokens"
                events += [{"type":"content_block_start", "index":0, "content_block":{"type":"tool_use", "id":"large-write", "name":"write", "input":{}}},
                           {"type":"content_block_delta", "index":0, "delta":{"type":"input_json_delta", "partial_json":json.dumps({"path":"large.txt", "content":PAYLOAD}) if enough else '{"path":"large.txt","content":"cut'}}]
            elif last == "truncate":
                stop = "max_tokens"
                events += [{"type":"content_block_delta", "index":0, "delta":{"type":"text_delta", "text":"PARTIAL-RESPONSE"}}]
            else:
                text = "LARGE-FILE-DONE" if isinstance(last, list) else "LIMIT-CHECK-DONE"
                events += [{"type":"content_block_delta", "index":0, "delta":{"type":"text_delta", "text":text}}]
            events += [{"type":"message_delta", "delta":{"stop_reason":stop}, "usage":{"output_tokens":12000}}, {"type":"message_stop"}]
        for event in events:
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()

class OutputLimits(unittest.TestCase):
    def test_saved_limit_and_cli_override_reach_both_native_apis(self):
        for adapter in ["anthropic-api", "openai-api"]:
            for override in [None, 96000]:
                with self.subTest(adapter=adapter, override=override), tempfile.TemporaryDirectory() as directory:
                    app = App(Path(directory), Provider, adapter=adapter, connection_settings="max_output_tokens=64000\n",
                              arguments=() if override is None else ("--max-output-tokens", str(override)))
                    try:
                        app.resize(35, 160)
                        app.send(b"settings\r")
                        app.wait(lambda s: "LIMIT-CHECK-DONE" in s and "out 12000" in s, "configured output request")
                        request = app.server.requests[0]
                        self.assertEqual(request["max_tokens" if adapter == "anthropic-api" else "max_output_tokens"], override or 64000)
                    finally:
                        app.close()

    def test_model_default_allows_a_large_generated_file_and_tool_continuation(self):
        with tempfile.TemporaryDirectory() as directory:
            app = App(Path(directory), Provider, adapter="anthropic-api")
            try:
                app.send(b"large-file\r")
                app.wait(lambda s: "LARGE-FILE-DONE" in s, "large generated file tool cycle")
                self.assertEqual((app.workspace / "large.txt").read_text(), PAYLOAD)
                self.assertTrue(all(r["max_tokens"] == 128000 for r in app.server.requests))
                self.assertEqual(len(app.server.requests), 2)
            finally:
                app.close()

    def test_truncation_is_visible_and_the_next_prompt_works(self):
        with tempfile.TemporaryDirectory() as directory:
            app = App(Path(directory), Provider, adapter="anthropic-api")
            try:
                app.resize(35, 160)
                app.send(b"truncate\r")
                app.wait(lambda s: "truncated" in s and "failed" in s and "out 12000" in s, "honest truncation failure and usage")
                app.send(b"continue\r")
                app.wait(lambda s: "LIMIT-CHECK-DONE" in s, "continuation after truncated output")
            finally:
                app.close()

if __name__ == "__main__":
    unittest.main()
