#!/usr/bin/env python3
"""Current-screen checks for the selected terminal usability sweep."""
import base64
import json
import re
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
from scrollback import App, Provider, row_numbers


def mouse(app, button, x, y, release=False):
    app.send(f"\x1b[<{button};{x};{y}{'m' if release else 'M'}".encode())


class SilentProvider(Provider):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        self.server.received = body
        self.server.more.wait(8)
        try:
            self.event({'type': 'response.output_text.delta', 'delta': 'FINISHED'})
            self.event({'type': 'response.completed', 'response': {'output': []}})
        except (BrokenPipeError, ConnectionResetError):
            pass


class UnicodeProvider(Provider):
    def do_POST(self):
        self.rfile.read(int(self.headers['Content-Length']))
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        self.event({'type':'response.output_text.delta', 'delta':'a界e\u0301z'})
        self.server.more.wait(8)
        try:
            self.event({'type':'response.output_text.delta', 'delta':' CHANGED'})
            self.event({'type':'response.completed', 'response':{'output':[]}})
        except (BrokenPipeError, ConnectionResetError):
            pass


class TerminalSweep(unittest.TestCase):
    def test_reverse_unicode_selection_survives_resize_in_real_terminal(self):
        with tempfile.TemporaryDirectory(prefix='demoncoder-unicode-') as directory:
            app = App(Path(directory), provider=UnicodeProvider)
            try:
                app.send(b'unicode-check\r')
                screen = app.wait(lambda s: 'a界' in s, 'Unicode output')
                y = next(i+1 for i,line in enumerate(screen.splitlines()) if 'a界' in line)
                mouse(app, 0, 8, y)
                mouse(app, 32, 4, y)
                mouse(app, 0, 4, y, True)
                app.wait(lambda s: 'Ctrl-Y copy' in s, 'Unicode selection')
                app.resize(20, 45)
                app.server.more.set()
                app.wait(lambda s: 'Prompt · Enter' in s, 'completion after resize')
                self.assertNotIn(b'\x1b]52;', app.output)
                app.send(b'\x19')
                deadline = time.monotonic() + 3
                match = None
                while time.monotonic() < deadline and match is None:
                    app.collect()
                    match = re.search(rb'\x1b\]52;c;([A-Za-z0-9+/=]*)(?:\x07|\x1b\\)', app.output)
                self.assertIsNotNone(match)
                self.assertEqual(base64.b64decode(match[1]).decode(), '界e\u0301z')
            finally:
                app.close()

    def test_indicator_advances_without_streamed_text_and_stops_after_completion(self):
        with tempfile.TemporaryDirectory(prefix='demoncoder-sweep-') as directory:
            app = App(Path(directory), provider=SilentProvider)
            try:
                app.resize(35, 60)
                app.send(b'silent-check\r')
                app.wait(lambda s: 'Working' in s, 'silent working state')
                headers = set()
                deadline = time.monotonic() + .7
                while time.monotonic() < deadline:
                    app.collect(.06)
                    headers.add(app.screen().splitlines()[0])
                self.assertGreater(len(headers), 1, 'working indicator never advanced')
                app.send(b'unsent-draft')
                app.wait(lambda s: 'unsent-draft' in s, 'input during silent wait')
                app.server.more.set()
                app.wait(lambda s: 'complete' in s, 'completed turn')
                before = app.screen().splitlines()[0]
                for _ in range(8):
                    app.collect(.04)
                self.assertEqual(before, app.screen().splitlines()[0])
            finally:
                app.close()

    def test_drag_scrollbar_preserves_history_when_output_arrives(self):
        with tempfile.TemporaryDirectory(prefix='demoncoder-sweep-') as directory:
            app = App(Path(directory))
            try:
                app.send(b'\x0fscroll-check\r')
                app.wait(lambda s: 'ROW-00399' in s, 'full scrollback')
                rail = app.columns - 2
                mouse(app, 0, rail, 29)
                mouse(app, 32, rail, 10)
                mouse(app, 0, rail, 10, True)
                older = app.wait(lambda s: bool(row_numbers(s)) and max(row_numbers(s)) < 399, 'dragged history')
                first = min(row_numbers(older))
                app.server.more.set()
                after = app.wait(lambda s: 'complete' in s.splitlines()[0], 'completion while anchored')
                self.assertEqual(first, min(row_numbers(after)))
                app.send(b'\x1b[F')
                app.wait(lambda s: 'NEWEST-MARKER' in s, 'return to live tail')
            finally:
                app.close()

    def test_selected_displayed_text_survives_new_output_and_copies_only_on_request(self):
        with tempfile.TemporaryDirectory(prefix='demoncoder-sweep-') as directory:
            app = App(Path(directory))
            try:
                app.send(b'\x0fscroll-check\r')
                app.wait(lambda s: 'ROW-00399' in s, 'full scrollback')
                lines = app.screen().splitlines()
                y = next(i + 1 for i, l in enumerate(lines) if 'ROW-' in l)
                expected = lines[y - 1][2:11]
                mouse(app, 0, 3, y)
                mouse(app, 32, 12, y)
                mouse(app, 0, 12, y, True)
                app.wait(lambda s: 'Ctrl-Y copy' in s, 'selection controls')
                self.assertNotIn(b'\x1b]52;', app.output)
                app.server.more.set()
                app.wait(lambda s: 'complete' in s.splitlines()[0], 'output while selected')
                app.send(b'\x19')
                deadline = time.monotonic() + 3
                match = None
                while time.monotonic() < deadline and match is None:
                    app.collect()
                    match = re.search(rb'\x1b\]52;c;([A-Za-z0-9+/=]*)(?:\x07|\x1b\\)', app.output)
                self.assertIsNotNone(match, 'explicit copy emitted no OSC 52 clipboard request')
                self.assertEqual(base64.b64decode(match[1]).decode(), expected)
                app.send(b'\x1b')
                app.wait(lambda s: 'Ctrl-Y copy' not in s, 'selection cleared')
            finally:
                app.close()


if __name__ == '__main__':
    unittest.main()
