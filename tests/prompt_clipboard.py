#!/usr/bin/env python3
"""Prompt-only copy shortcuts and terminal-delivered paste through a PTY."""
import base64
import re
import sys
import tempfile
import time
import unittest
from pathlib import Path
sys.dont_write_bytecode = True
from scrollback import App
from terminal_sweep import SilentProvider


class PromptClipboard(unittest.TestCase):
    def test_copy_uses_prompt_text_without_submitting_or_cancelling(self):
        with tempfile.TemporaryDirectory() as directory:
            app = App(Path(directory), provider=SilentProvider)
            try:
                app.resize(35, 180)
                app.send(b'silent-check\r')
                app.wait(lambda s: 'Working' in s, 'active turn')
                app.send(b'\x1b[99;6u')  # an empty prompt must not cancel
                app.send('draft 界'.encode())
                app.wait(lambda s: 'draft 界' in s, 'prompt draft')
                self.assertNotIn(b'\x1b]52;', app.output)
                app.send(b'\x1b[99;6u')  # forwarded Ctrl+Shift+C
                deadline = time.monotonic() + 3
                match = None
                while time.monotonic() < deadline and match is None:
                    app.collect()
                    match = re.search(rb'\x1b\]52;c;([A-Za-z0-9+/=]*)(?:\x07|\x1b\\)', app.output)
                self.assertIsNotNone(match, 'prompt copy emitted no clipboard request')
                self.assertEqual(base64.b64decode(match[1]).decode(), 'draft 界')
                self.assertIn('Working', app.screen())
                self.assertIn('draft 界', app.screen())
                self.assertIn('Ctrl-Shift-C/V', app.screen())
            finally:
                app.close()

    def test_terminal_paste_is_editor_text_and_modes_are_restored(self):
        with tempfile.TemporaryDirectory() as directory:
            app = App(Path(directory), provider=SilentProvider)
            try:
                self.assertIn(b'\x1b[?2004h', app.output, 'bracketed paste is not enabled')
                app.send(b'\x1b[200~first\nsecond\x03\x11\x1b[201~')
                app.wait(lambda s: 'firstsecond' in s, 'pasted editor text')
                self.assertFalse(hasattr(app.server, 'received'), 'paste submitted a prompt')
                self.assertIsNone(app.process.poll(), 'pasted control text quit the application')
                self.assertIn('Prompt', app.screen())
            finally:
                app.close()
            self.assertIn(b'\x1b[?2004l', app.output, 'bracketed paste was not disabled')
            self.assertEqual(app.process.returncode, 0)


if __name__ == '__main__':
    unittest.main()
