#!/usr/bin/python3
"""Run the actual installed CLI with disposable authentication and local routing."""
import json
import os
from pathlib import Path
import subprocess
import sys
import signal

root = Path.cwd()
config = json.loads((root / "backend.json").read_text())
(root / "relay.pid").write_text(str(os.getpid()))
def disconnect(*_):
    os.close(sys.stdout.fileno())
    (root / "relay-disconnected").write_text("stdout closed")

signal.signal(signal.SIGUSR1, disconnect)
env = {"PATH": "/usr/bin:/bin", "HOME": str(root / "home"), "CLAUDE_CONFIG_DIR": str(root / "home"), "CLAUDE_CODE_OAUTH_TOKEN": "synthetic-oauth", "ANTHROPIC_BASE_URL": config["endpoint"], "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "DISABLE_TELEMETRY": "1", "DISABLE_ERROR_REPORTING": "1", "DISABLE_AUTOUPDATER": "1"}
with (root / "backend-stderr.txt").open("wb") as errors, (root / "backend-wire.jsonl").open("wb") as wire:
    child = subprocess.Popen([config["binary"], *sys.argv[1:]], env=env, stdin=sys.stdin, stdout=subprocess.PIPE, stderr=errors)
    (root / "backend.pid").write_text(str(child.pid))
    for line in child.stdout:
        wire.write(line)
        wire.flush()
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
    raise SystemExit(child.wait())
