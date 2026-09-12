#!/usr/bin/python3
"""Local model peer for real Claude compaction; no provider calls or credentials."""
import http.server
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
automatic = sys.argv[2] == "auto"


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        if not self.path.split("?")[0].endswith("/messages"):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"input_tokens":1}')
            return
        self.server.count += 1
        with (root / "model-requests.jsonl").open("a") as output:
            output.write(json.dumps(body) + "\n")
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        events = [
            {"type": "message_start", "message": {"id": "msg_fixture_" + str(self.server.count), "type": "message", "role": "assistant", "model": "claude-sonnet-4-6", "content": [], "stop_reason": None, "usage": {"input_tokens": 195000 if automatic and self.server.count == 4 else 30000, "output_tokens": 0}}},
            {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
            {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Local fixture summary: prior user requested hello and assistant answered hello."}},
            {"type": "content_block_stop", "index": 0},
            {"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": None}, "usage": {"output_tokens": 20}},
            {"type": "message_stop"},
        ]
        for event in events:
            self.wfile.write(("event: " + event["type"] + "\ndata: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
server.count = 0
print(server.server_port, flush=True)
server.serve_forever()
