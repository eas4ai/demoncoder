#!/usr/bin/python3
"""Redirect an installed backend to disposable local credentials/model fixtures."""
import json
import os
from pathlib import Path
import sys
import subprocess

config = json.loads(Path("installed-backend.json").read_text())
if config["adapter"] == "claude":
    os.environ["ANTHROPIC_BASE_URL"] = config["endpoint"]
    os.environ["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"] = "1"
    os.environ["DISABLE_TELEMETRY"] = "1"
    os.environ["DISABLE_ERROR_REPORTING"] = "1"
    os.environ["DISABLE_AUTOUPDATER"] = "1"
with open("installed-stderr.txt", "wb") as errors, open("installed-wire.jsonl", "wb") as wire:
    child = subprocess.Popen([config["binary"], *sys.argv[1:]], stdin=sys.stdin, stdout=subprocess.PIPE, stderr=errors)
    for line in child.stdout:
        wire.write(line)
        wire.flush()
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
    raise SystemExit(child.wait())
