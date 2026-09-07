#!/usr/bin/env python3
"""Status-strip and current-context checks through the production terminal."""
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
sys.dont_write_bytecode = True
from scrollback import App, Provider

class CountingProvider(Provider):
    def do_POST(self):
        self.server.requests = getattr(self.server, "requests", 0) + 1
        super().do_POST()


def prepare(workspace, _home):
    def git(*args):
        subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgsign=false', *args], cwd=workspace, check=True, capture_output=True)
    git('init', '-q', '-b', 'sweep-branch')
    (workspace / 'tracked.txt').write_text('before\n')
    git('add', 'tracked.txt'); git('commit', '-qm', 'seed')
    (workspace / 'tracked.txt').write_text('after\n')
    (workspace / 'untracked space.txt').write_text('new\n')


class StatusSweep(unittest.TestCase):
    def test_hung_git_does_not_block_input_or_quit(self):
        def hung_git(workspace, home):
            prepare(workspace, home)
            config = workspace / '.git/config'
            config.unlink()
            os.mkfifo(config)
        with tempfile.TemporaryDirectory(prefix='demoncoder-git-timeout-') as directory:
            app = App(Path(directory), prepare=hung_git)
            try:
                app.resize(35, 220)
                app.send(b'input-still-works')
                app.wait(lambda s: 'input-still-works' in s, 'input while Git is hung', timeout=1)
                app.wait(lambda s: 'Git unavailable' in s, 'bounded Git timeout', timeout=4)
            finally:
                app.close()
            self.assertEqual(app.process.returncode, 0)

    def test_status_order_and_selected_workspace_git(self):
        with tempfile.TemporaryDirectory(prefix='demoncoder-status-') as directory:
            app = App(Path(directory), prepare=prepare)
            try:
                app.resize(35, 220)
                screen = app.wait(lambda s: '2 dirty sweep-branch' in s, 'selected-workspace Git status')
                strip = screen.splitlines()[-2]
                fields = ['fixture-model', 'Ctx ', '2 dirty sweep-branch', '+1/-1', 'agents 0']
                positions = [strip.index(f) for f in fields]
                self.assertEqual(positions, sorted(positions))
                self.assertNotIn('cost', strip)
                self.assertNotIn('in unknown', strip)
                app.send(b'scroll-check\r')
                app.wait(lambda s: 'ROW-00399' in s, 'streamed text')
                app.server.more.set()
                result = app.wait(lambda s: 'in 12345' in s, 'reported usage')
                strip = result.splitlines()[-2]
                self.assertLess(strip.index('agents 0'), strip.index('in 12345'))
                self.assertIn('Ctx 12354/?', strip)
                self.assertNotIn('cost', strip)
            finally:
                app.close()

    def test_explicit_context_capacity_and_latest_request_are_not_accumulated(self):
        with tempfile.TemporaryDirectory(prefix='demoncoder-context-') as directory:
            app = App(Path(directory), provider=CountingProvider, arguments=('--context-window', '20000'))
            try:
                app.resize(35, 220)
                app.wait(lambda s: 'Ctx ?/20000' in s, 'explicit capacity')
                app.server.more.set()
                for turn in range(1, 3):
                    app.send(b'scroll-check\r')
                    app.wait(lambda s: getattr(app.server, 'requests', 0) == turn and 'complete' in s and '12354/20000' in s, 'current request occupancy')
                    self.assertIn('7646 free', app.screen())
                    app.send(b'\x0f')
                    app.collect(.05)
                self.assertNotIn('24708', app.screen())
            finally:
                app.close()


if __name__ == '__main__':
    unittest.main()
