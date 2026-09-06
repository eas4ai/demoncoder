#!/usr/bin/env python3
"""Production CLI and startup regressions, using disposable homes and projects."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

sys.dont_write_bytecode = True
from onboarding import App, complete_setup
from terminal_session import BINARY


class Information(unittest.TestCase):
    def test_help_and_version_do_not_start_setup(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-info-") as directory:
            root = Path(directory)
            env = {"PATH":"/usr/bin:/bin", "HOME":str(root)}
            versions = []
            for flag in ("--help", "-h", "--version", "-V", "-v"):
                result = subprocess.run([str(BINARY), flag], cwd=root, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, (flag, result.stderr))
                self.assertNotIn("setup", result.stderr)
                if flag in ("--help", "-h"):
                    self.assertIn("Usage: demoncoder [OPTIONS]", result.stdout)
                    self.assertIn("--workspace", result.stdout)
                else:
                    versions.append(result.stdout)
            self.assertEqual(len(set(versions)), 1)
            self.assertTrue(versions[0].startswith("demoncoder "))
            self.assertEqual(list(root.iterdir()), [])


class Workspace(unittest.TestCase):
    def test_default_workspace_is_launch_directory_and_explicit_overrides(self):
        for explicit in (False, True):
            with self.subTest(explicit=explicit), tempfile.TemporaryDirectory(prefix="demoncoder-cwd-") as directory:
                root = Path(directory)
                app = App(root, omit_workspace=not explicit)
                try:
                    complete_setup(app)
                    settings = tomllib.loads(app.settings.read_text())
                    self.assertEqual(settings["trusted_workspaces"], [str(app.workspace.resolve())])
                    os.write(app.master, b"workspace-check\r")
                    app.wait("RECEIVED-workspace-check")
                    self.assertEqual(json.loads((app.workspace / "received-prompt.json").read_text()), "workspace-check")
                    app.finish()
                finally:
                    app.close()


class Directory(unittest.TestCase):
    def test_existing_default_directory_is_secured_without_losing_files(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-directory-") as directory:
            root = Path(directory)
            parent = root / "home/.demoncoder"
            parent.mkdir(parents=True)
            parent.chmod(0o775)
            canary = parent / "legacy-config.toml"
            original = b'legacy = "must remain unchanged"\n'
            canary.write_bytes(original)
            app = App(root)
            try:
                complete_setup(app)
                self.assertEqual(parent.stat().st_mode & 0o777, 0o700)
                self.assertEqual(canary.read_bytes(), original)
                self.assertEqual(app.settings.stat().st_mode & 0o777, 0o600)
                app.finish()
            finally:
                app.close()

    def test_symlinked_default_directory_is_rejected_without_chmod(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-directory-link-") as directory:
            root = Path(directory)
            home = root / "home"
            home.mkdir()
            outside = root / "outside"
            outside.mkdir()
            outside.chmod(0o775)
            (home / ".demoncoder").symlink_to(outside, target_is_directory=True)
            app = App(root)
            try:
                app.process.wait(timeout=5)
                self.assertNotEqual(app.process.returncode, 0)
                self.assertEqual(outside.stat().st_mode & 0o777, 0o775)
                self.assertEqual(list(outside.iterdir()), [])
                self.assertFalse(app.log.exists())
            finally:
                app.close()

    def test_custom_shared_parent_is_rejected_without_chmod(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-custom-parent-") as directory:
            root = Path(directory)
            custom = root / "shared"
            custom.mkdir()
            custom.chmod(0o775)
            config = custom / "settings.toml"
            config.write_text('default_connection="codex"\n[connections.codex]\nadapter="codex"\n')
            before = config.read_bytes()
            app = App(root, setup=True, config=config)
            try:
                app.process.wait(timeout=5)
                self.assertNotEqual(app.process.returncode, 0)
                self.assertEqual(custom.stat().st_mode & 0o777, 0o775)
                self.assertEqual(config.read_bytes(), before)
                self.assertFalse((custom / "settings.toml.lock").exists())
                self.assertFalse(app.log.exists())
            finally:
                app.close()


if __name__ == "__main__":
    passed = True
    for requirement, case in (("START-001", Information), ("START-002", Workspace), ("START-003", Directory)):
        result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(case))
        print(f"cairn: {requirement}: " + ("pass" if result.wasSuccessful() else "fail"), flush=True)
        passed &= result.wasSuccessful()
    raise SystemExit(not passed)
