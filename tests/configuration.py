#!/usr/bin/env python3
"""Configuration failures must stop before terminal startup without exposing keys."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/demoncoder"
SECRET = "synthetic-secret-must-not-appear"


class Configuration(unittest.TestCase):
    def test_rejected_inputs(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-config-") as directory:
            home = Path(directory)
            config = home / ".demoncoder/settings.toml"
            config.parent.mkdir()
            base = 'default_connection="selected"\n[connections.selected]\nadapter="openai-api"\nmodel="fixture"\n'
            cases = [
                (base + f'api_key="{SECRET}"\n', 0o644, "owner-only"),
                (base + f'unknown="{SECRET}"\n', 0o600, "invalid connection"),
                (base + f'api_key="{SECRET}', 0o600, "invalid connection"),
                (base + f'api_key="{SECRET}"\neffort="unsupported"\n', 0o600, "unsupported reasoning effort"),
                (base.replace('"openai-api"', '"claude"') + f'api_key="{SECRET}"\n', 0o600, "subscription connections do not accept"),
                (base + '#' + 'x' * (64 * 1024), 0o600, "exceeds 64 KiB"),
                (base, 0o600, "set OPENAI_API_KEY or api_key"),
            ]
            env = {"PATH":"/usr/bin:/bin", "HOME":directory}
            command = [str(BINARY), "--workspace", directory]
            for content, mode, message in cases:
                with self.subTest(message=message):
                    config.write_text(content)
                    config.chmod(mode)
                    result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=5)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(message, result.stderr)
                    self.assertNotIn(SECRET, result.stdout + result.stderr)
            config.write_text(base + f'api_key="{SECRET}"\n')
            env["OPENAI_API_KEY"] = " "
            result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=5)
            self.assertIn("OPENAI_API_KEY is empty", result.stderr)
            self.assertNotIn(SECRET, result.stderr)
            config.unlink()
            os.mkfifo(config)
            result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=5)
            self.assertIn("regular file", result.stderr)


if __name__ == "__main__":
    unittest.main()
