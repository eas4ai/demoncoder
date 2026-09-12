#!/usr/bin/python3
"""Keep installed Codex on its built-in route while serving local model traffic."""

import json
import os
from pathlib import Path
import shutil
import socketserver
import ssl
import subprocess
import sys
import threading
import time
from urllib.parse import urlsplit


class _ConnectHandler(socketserver.StreamRequestHandler):
    def handle(self):
        self.phase = "request"
        try:
            self._handle_connect()
        except (ssl.SSLError, OSError) as error:
            if self.phase == "TLS handshake" and type(error) in (
                ssl.SSLEOFError,
                ConnectionResetError,
            ):
                self.server.record_handshake_abort(error)
                return
            # Exception messages can contain peer data. Keep diagnostics bounded.
            self.server.errors.append(
                f"Codex HTTPS fixture failed during {self.phase}: "
                f"{type(error).__name__} errno={error.errno}"
            )

    def _handle_connect(self):
        request = self.rfile.readline(8192)
        if not request:
            # Clients can open a pooled connection and close it without using it.
            return
        if not request.endswith(b"\n"):
            kind = "oversized" if len(request) == 8192 else "truncated"
            self.server.errors.append(f"{kind} HTTPS proxy request")
            return
        total = len(request)
        self.phase = "headers"
        while True:
            header = self.rfile.readline(8192)
            total += len(header)
            if total > 64 * 1024 or (
                len(header) == 8192 and not header.endswith(b"\n")
            ):
                self.server.errors.append("oversized HTTPS proxy headers")
                return
            if not header.endswith(b"\n"):
                self.server.errors.append("truncated HTTPS proxy headers")
                return
            if header in (b"\r\n", b"\n"):
                break
        try:
            method, target, _ = request.decode("ascii").strip().split()
        except (UnicodeDecodeError, ValueError):
            self.server.errors.append("invalid HTTPS proxy request")
            return
        self.server.connect_targets.append(target)
        self.phase = "CONNECT reply"
        if method != "CONNECT" or target != "chatgpt.com:443":
            self.server.errors.append("unexpected HTTPS proxy target or method")
            self.wfile.write(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")
            self.wfile.flush()
            return
        self.wfile.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        self.wfile.flush()
        self.phase = "TLS handshake"
        stream = self.server.tls_context.wrap_socket(self.connection, server_side=True)
        self.phase = "model handler"
        stream.settimeout(None)
        self.server.model_handler(stream, self.client_address, self.server)


class CodexHttpsServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True

    drain_timeout = 5.0

    def __init__(self, *args, **kwargs):
        self.errors = []
        self._handlers = threading.Condition()
        self._active_handlers = 0
        self._abort_lock = threading.Lock()
        self._abort_count = 0
        super().__init__(*args, **kwargs)

    def get_request(self):
        request, address = super().get_request()
        request.settimeout(5.0)
        return request, address

    def process_request(self, request, client_address):
        # Register before starting the thread so close cannot miss a new handler.
        with self._handlers:
            self._active_handlers += 1
        try:
            super().process_request(request, client_address)
        except BaseException:
            with self._handlers:
                self._active_handlers -= 1
                self._handlers.notify_all()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            with self._handlers:
                self._active_handlers -= 1
                self._handlers.notify_all()

    def server_close(self):
        super().server_close()
        with self._handlers:
            settled = self._handlers.wait_for(
                lambda: self._active_handlers == 0, timeout=self.drain_timeout
            )
            if not settled:
                self.errors.append(
                    "Codex HTTPS fixture has unsettled connection handlers"
                )

    def record_handshake_abort(self, error):
        # Only transport observations: no assertion about why a peer disconnected.
        with self._abort_lock:
            if self._abort_count >= 128:
                if self._abort_count == 128:
                    self.errors.append(
                        "Codex HTTPS fixture abort journal limit exceeded"
                    )
                    self._abort_count += 1
                return
            self._abort_count += 1
            record = {
                "sequence": self._abort_count,
                "monotonic_ns": time.monotonic_ns(),
                "phase": "TLS handshake",
                "type": type(error).__name__,
                "errno": error.errno,
            }
            try:
                with self.abort_journal.open("a", encoding="utf-8") as journal:
                    journal.write(json.dumps(record) + "\n")
                    journal.flush()
                    os.fsync(journal.fileno())
            except OSError as failure:
                self.errors.append(
                    "Codex HTTPS fixture abort journal write failed: "
                    f"{type(failure).__name__} errno={failure.errno}"
                )

    def handle_error(self, _request, _client_address):
        error = sys.exception()
        self.errors.append(
            f"Codex HTTPS fixture handler failed: {type(error).__name__}"
        )


def create_server(directory, model_handler):
    """Create a loopback CONNECT proxy with a one-run ChatGPT certificate."""
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    openssl = shutil.which("openssl")
    if openssl is None:
        raise RuntimeError("prerequisite missing: openssl")
    ca_key = directory / "ca.key"
    ca_certificate = directory / "ca.pem"
    leaf_key = directory / "chatgpt.key"
    leaf_request = directory / "chatgpt.csr"
    leaf_certificate = directory / "chatgpt.pem"
    leaf_extensions = directory / "chatgpt.ext"
    leaf_extensions.write_text(
        "subjectAltName=DNS:chatgpt.com\n"
        "basicConstraints=critical,CA:FALSE\n"
        "keyUsage=critical,digitalSignature,keyEncipherment\n"
        "extendedKeyUsage=serverAuth\n"
    )
    quiet = {
        "stdout": subprocess.DEVNULL,
        "stderr": subprocess.DEVNULL,
        "check": True,
        "timeout": 10,
    }
    subprocess.run(
        [
            openssl,
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            str(ca_key),
            "-out",
            str(ca_certificate),
            "-days",
            "1",
            "-subj",
            "/CN=DemonCoder installed Codex fixture CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ],
        **quiet,
    )
    subprocess.run(
        [
            openssl,
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            str(leaf_key),
            "-out",
            str(leaf_request),
            "-subj",
            "/CN=chatgpt.com",
        ],
        **quiet,
    )
    subprocess.run(
        [
            openssl,
            "x509",
            "-req",
            "-in",
            str(leaf_request),
            "-CA",
            str(ca_certificate),
            "-CAkey",
            str(ca_key),
            "-CAcreateserial",
            "-out",
            str(leaf_certificate),
            "-days",
            "1",
            "-sha256",
            "-extfile",
            str(leaf_extensions),
        ],
        **quiet,
    )
    server = CodexHttpsServer(("127.0.0.1", 0), _ConnectHandler)
    server.abort_journal = directory / "connection-aborts.jsonl"
    try:
        server.abort_journal.touch(exist_ok=False)
    except OSError:
        server.server_close()
        raise
    server.server_port = server.server_address[1]
    server.ca_certificate = ca_certificate
    server.connect_targets = []
    server.errors = []
    server.model_handler = model_handler
    server.tls_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    server.tls_context.minimum_version = ssl.TLSVersion.TLSv1_2
    server.tls_context.set_alpn_protocols(["http/1.1"])
    server.tls_context.load_cert_chain(leaf_certificate, leaf_key)
    return server


def _launch():
    configuration = json.loads(Path("installed-backend.json").read_text())
    if configuration.get("adapter") != "codex":
        raise RuntimeError("Codex HTTPS launcher received another adapter")
    proxy = urlsplit(configuration["https_proxy"])
    if proxy.scheme != "http" or proxy.hostname != "127.0.0.1" or proxy.port is None:
        raise RuntimeError("Codex HTTPS fixture requires a loopback proxy")
    ca_certificate = Path(configuration["ca_certificate"])
    if not ca_certificate.is_file():
        raise RuntimeError("Codex HTTPS fixture CA is unavailable")
    for name in [
        "HTTP_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
        "CODEX_CA_CERTIFICATE",
        "SSL_CERT_FILE",
    ]:
        os.environ.pop(name, None)
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"]:
        os.environ[name] = configuration["https_proxy"]
    os.environ["CODEX_CA_CERTIFICATE"] = str(ca_certificate)
    os.environ["NO_PROXY"] = ""
    launcher = Path(__file__).with_name("installed_backend_launcher.py")
    os.execv(sys.executable, [sys.executable, str(launcher), *sys.argv[1:]])


if __name__ == "__main__":
    _launch()
