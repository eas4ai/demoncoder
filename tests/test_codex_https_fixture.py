import importlib.util
import io
import json
import socketserver
import tempfile
import threading
import time
from pathlib import Path
import ssl
import unittest
from types import SimpleNamespace
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location(
    "fixture", Path(__file__).with_name("codex_https_fixture.py")
)
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class ConnectHandlerTests(unittest.TestCase):
    def handler(self, data):
        handler = fixture._ConnectHandler.__new__(fixture._ConnectHandler)
        handler.rfile = io.BytesIO(data)
        handler.wfile = io.BytesIO()
        handler.connection = object()
        handler.client_address = ("127.0.0.1", 1234)
        handler.server = SimpleNamespace(
            errors=[], connect_targets=[], tls_context=Mock(), model_handler=Mock()
        )
        return handler

    def test_empty_connection_is_unused(self):
        handler = self.handler(b"")
        handler.handle()
        self.assertEqual(handler.server.errors, [])
        handler.server.tls_context.wrap_socket.assert_not_called()
        handler.server.model_handler.assert_not_called()

    def test_malformed_connections_fail_before_tls(self):
        cases = [
            (b"CONNECT chatgpt.com:443", "truncated HTTPS proxy request"),
            (b"x" * 8192, "oversized HTTPS proxy request"),
            (b"CONNECT chatgpt.com:443 HTTP/1.1\r\n", "truncated HTTPS proxy headers"),
            (
                b"CONNECT chatgpt.com:443 HTTP/1.1\r\nHost: secret",
                "truncated HTTPS proxy headers",
            ),
            (
                b"CONNECT chatgpt.com:443 HTTP/1.1\r\n" + b"x" * 8192,
                "oversized HTTPS proxy headers",
            ),
            (
                b"CONNECT chatgpt.com:443 HTTP/1.1\r\n" + (b"x" * 8000 + b"\r\n") * 9,
                "oversized HTTPS proxy headers",
            ),
        ]
        for data, error in cases:
            with self.subTest(error=error, size=len(data)):
                handler = self.handler(data)
                handler.handle()
                self.assertEqual(handler.server.errors, [error])
                handler.server.tls_context.wrap_socket.assert_not_called()
                handler.server.model_handler.assert_not_called()

    def test_wrong_target_stays_failure(self):
        handler = self.handler(b"CONNECT example.com:443 HTTP/1.1\r\n\r\n")
        handler.handle()
        self.assertTrue(handler.server.errors)
        self.assertTrue(handler.wfile.getvalue().startswith(b"HTTP/1.1 403"))
        handler.server.tls_context.wrap_socket.assert_not_called()

    def test_valid_connect_reaches_model(self):
        handler = self.handler(b"CONNECT chatgpt.com:443 HTTP/1.1\r\n\r\n")
        handler.handle()
        self.assertEqual(handler.server.errors, [])
        self.assertEqual(handler.server.connect_targets, ["chatgpt.com:443"])
        handler.server.tls_context.wrap_socket.return_value.settimeout.assert_called_once_with(
            None
        )
        handler.server.model_handler.assert_called_once_with(
            handler.server.tls_context.wrap_socket.return_value,
            handler.client_address,
            handler.server,
        )

    def test_socket_failures_record_phase_without_sensitive_text(self):
        for phase in (
            "request",
            "headers",
            "CONNECT reply",
            "model handler",
        ):
            with self.subTest(phase=phase):
                handler = self.handler(b"CONNECT chatgpt.com:443 HTTP/1.1\r\n\r\n")
                error = ConnectionResetError(104, "secret response body")
                if phase == "request":
                    handler.rfile = Mock()
                    handler.rfile.readline.side_effect = error
                elif phase == "headers":
                    handler.rfile = Mock()
                    handler.rfile.readline.side_effect = [
                        b"CONNECT chatgpt.com:443 HTTP/1.1\r\n",
                        error,
                    ]
                elif phase == "CONNECT reply":
                    handler.wfile = Mock()
                    handler.wfile.write.side_effect = error
                else:
                    handler.server.model_handler.side_effect = error
                handler.handle()
                self.assertEqual(len(handler.server.errors), 1)
                self.assertIn(phase, handler.server.errors[0])
                self.assertNotIn("secret", handler.server.errors[0])
                self.assertLess(len(handler.server.errors[0]), 160)


class AbandonedHandshakeTests(ConnectHandlerTests):
    def server(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        server = fixture.CodexHttpsServer(("127.0.0.1", 0), fixture._ConnectHandler)
        self.addCleanup(server.server_close)
        server.errors = []
        server.abort_journal = Path(directory.name) / "connection-aborts.jsonl"
        return server

    def test_exact_handshake_aborts_are_persisted_without_model_calls(self):
        server = self.server()
        for error in (
            ssl.SSLEOFError(8, "secret"),
            ConnectionResetError(104, "secret"),
        ):
            handler = self.handler(b"CONNECT chatgpt.com:443 HTTP/1.1\r\n\r\n")
            handler.server.record_handshake_abort = server.record_handshake_abort
            handler.server.tls_context.wrap_socket.side_effect = error
            handler.handle()
            self.assertEqual(handler.server.errors, [])
            handler.server.model_handler.assert_not_called()
        records = [
            json.loads(line) for line in server.abort_journal.read_text().splitlines()
        ]
        self.assertEqual([r["sequence"] for r in records], [1, 2])
        self.assertEqual(
            [r["type"] for r in records], ["SSLEOFError", "ConnectionResetError"]
        )
        self.assertTrue(all(r["phase"] == "TLS handshake" for r in records))
        self.assertLessEqual(records[0]["monotonic_ns"], records[1]["monotonic_ns"])
        self.assertNotIn("secret", server.abort_journal.read_text())
        self.assertEqual(server.errors, [])

    def test_other_tls_errors_and_subclasses_remain_failures(self):
        class DerivedEOF(ssl.SSLEOFError):
            pass

        for kind in (ssl.SSLError, ssl.SSLCertVerificationError, DerivedEOF):
            handler = self.handler(b"CONNECT chatgpt.com:443 HTTP/1.1\r\n\r\n")
            handler.server.record_handshake_abort = Mock()
            handler.server.tls_context.wrap_socket.side_effect = kind(1, "secret")
            handler.handle()
            self.assertEqual(len(handler.server.errors), 1)
            handler.server.record_handshake_abort.assert_not_called()
            handler.server.model_handler.assert_not_called()

    def test_ssl_eof_outside_handshake_remains_failure(self):
        for phase in ("request", "headers", "CONNECT reply", "model handler"):
            handler = self.handler(b"CONNECT chatgpt.com:443 HTTP/1.1\r\n\r\n")
            handler.server.record_handshake_abort = Mock()
            error = ssl.SSLEOFError(8, "secret")
            if phase == "request":
                handler.rfile = Mock()
                handler.rfile.readline.side_effect = error
            elif phase == "headers":
                handler.rfile = Mock()
                handler.rfile.readline.side_effect = [
                    b"CONNECT chatgpt.com:443 HTTP/1.1\r\n",
                    error,
                ]
            elif phase == "CONNECT reply":
                handler.wfile = Mock()
                handler.wfile.flush.side_effect = error
            else:
                handler.server.model_handler.side_effect = error
            handler.handle()
            self.assertEqual(len(handler.server.errors), 1)
            self.assertIn(phase, handler.server.errors[0])
            handler.server.record_handshake_abort.assert_not_called()

    def test_journal_overflow_and_write_failure_are_errors(self):
        server = self.server()
        for _ in range(256):
            server.record_handshake_abort(ConnectionResetError(104, "secret"))
        self.assertEqual(len(server.abort_journal.read_text().splitlines()), 128)
        self.assertEqual(len(server.errors), 1)
        self.assertIn("limit", server.errors[0])
        server = self.server()
        with patch.object(Path, "open", side_effect=OSError("secret")):
            server.record_handshake_abort(ssl.SSLEOFError(8, "secret"))
        self.assertEqual(len(server.errors), 1)
        self.assertNotIn("secret", server.errors[0])

    def test_server_close_settles_late_handler_diagnostics(self):
        server = self.server()
        entered = threading.Event()
        release = threading.Event()

        def finish(_request, _address):
            entered.set()
            release.wait(1)
            server.errors.append("late handler failure")

        with patch.object(
            socketserver.ThreadingMixIn, "process_request_thread", side_effect=finish
        ):
            server.process_request(Mock(), ("127.0.0.1", 1234))
            self.assertTrue(entered.wait(1))
            timer = threading.Timer(0.03, release.set)
            timer.start()
            server.server_close()
            self.assertEqual(server.errors, ["late handler failure"])
            timer.join()

    def test_server_close_deadline_fails_unsettled_handlers(self):
        server = self.server()
        server.drain_timeout = 0.01
        entered = threading.Event()
        release = threading.Event()

        def finish(_request, _address):
            entered.set()
            release.wait(1)

        with patch.object(
            socketserver.ThreadingMixIn, "process_request_thread", side_effect=finish
        ):
            server.process_request(Mock(), ("127.0.0.1", 1234))
            self.assertTrue(entered.wait(1))
            started = time.monotonic()
            try:
                server.server_close()
                self.assertLess(time.monotonic() - started, 0.5)
                self.assertTrue(any("unsettled" in error for error in server.errors))
            finally:
                release.set()


if __name__ == "__main__":
    unittest.main()
