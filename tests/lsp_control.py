#!/usr/bin/python3
"""Synthetic test barriers and observations, independent of the LSP file view."""
import argparse
import http.server
import secrets
from pathlib import Path


NAMES = frozenset({
    ".fixture-mode", ".fixture-canaries", "release-unversioned", "release-exit",
    "release-request", "release-probe", "request-started", "descendant-escaped", "server-exiting",
    "adversarial-results", "save-receipts", "synchronized-version",
})


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def target(self):
        prefix = "/" + self.server.token + "/"
        name = self.path.removeprefix(prefix)
        if not self.path.startswith(prefix) or name not in NAMES:
            self.send_error(403)
            return None
        return self.server.directory / name

    def do_GET(self):
        target = self.target()
        if target is None:
            return
        try:
            data = target.read_bytes()
        except FileNotFoundError:
            self.send_error(404)
            return
        if len(data) > 32768:
            self.send_error(413)
            return
        self.send_response(200)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        target = self.target()
        if target is None:
            return
        length = int(self.headers.get("Content-Length", "0"))
        if not 0 <= length <= 32768:
            self.send_error(413)
            return
        data = self.rfile.read(length)
        if self.headers.get("X-Append") == "true":
            with target.open("ab") as output:
                output.write(data)
        else:
            target.write_bytes(data)
        self.send_response(204)
        self.send_header("Content-Length", "0")
        self.end_headers()


def server(directory):
    result = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    result.directory = Path(directory)
    result.token = secrets.token_hex(16)
    result.url = f"http://127.0.0.1:{result.server_port}/{result.token}/"
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    with server(args.directory) as control:
        print(control.url, flush=True)
        control.serve_forever()
