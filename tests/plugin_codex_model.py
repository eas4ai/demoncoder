#!/usr/bin/python3
"""Synthetic HTTPS model for actual managed Codex manual/automatic compaction."""
import http.server
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
from codex_https_fixture import create_server
from installed_backends import fake_codex_auth

root = Path(sys.argv[1])
automatic = sys.argv[2] == "auto"
home = root / "codex-home"
home.mkdir()
fake_codex_auth(home)
(home / "config.toml").write_text('model = "gpt-5.4"\ncli_auth_credentials_store = "file"\n' + ('model_auto_compact_token_limit = 200\n' if automatic else '') + '[features]\nenable_request_compression = false\n')


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(b'{"models":[],"items":[]}')

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
        self.send_response(200)
        if not self.path.endswith("/responses"):
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{}')
            return
        compact = json.loads(body.get("client_metadata", {}).get("x-codex-turn-metadata", "{}")).get("request_kind") == "compaction"
        self.server.count += 1
        with (root / "model-requests.jsonl").open("a") as output:
            output.write(json.dumps({"compact": compact, "body": body}) + "\n")
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        item = {"type": "compaction", "encrypted_content": "SYNTHETIC COMPACTED CONTEXT"} if compact else {"type": "message", "id": "msg_fixture", "role": "assistant", "content": [{"type": "output_text", "text": "HELLO"}]}
        tokens = 500 if automatic and self.server.count == 1 else 10
        events = [
            {"type": "response.created", "response": {"id": "resp_fixture", "status": "in_progress", "output": []}},
            {"type": "response.output_item.added", "output_index": 0, "item": item},
            {"type": "response.output_item.done", "output_index": 0, "item": item},
            {"type": "response.completed", "response": {"id": "resp_fixture", "status": "completed", "output": [item], "usage": {"input_tokens": tokens, "output_tokens": 5, "total_tokens": tokens + 5}}},
        ]
        for event in events:
            self.wfile.write(("event: " + event["type"] + "\ndata: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()


server = create_server(root / "tls", Model)
server.count = 0
print(json.dumps({"port": server.server_port, "ca": str(server.ca_certificate)}), flush=True)
server.serve_forever()
