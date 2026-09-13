#!/usr/bin/python3
"""Run the managed artifact against isolated local TLS and synthetic login."""
import json
import hashlib
import shlex
import os
from pathlib import Path
import subprocess
import sys
workspace = Path.cwd()
artifact_root = os.environ.pop("DEMONCODER_TEST_BACKEND_ROOT", None)
root = Path(artifact_root) if artifact_root else workspace
config = json.loads((root / "backend.json").read_text())
if "--demoncoder-compaction-capability" in sys.argv:
    os.execv(config["binary"], [config["binary"], *sys.argv[1:]])
if config.get("fault"):
    requirement = json.loads(os.environ["CODEX_DEMONCODER_COMPACTION_RELAY"])
    source = Path(requirement["source_path"])
    hooks = json.loads(source.read_text())
    command = shlex.join(["/usr/bin/python3", str(Path(__file__).with_name("plugin_codex_fault_relay.py")), config["fault"], *shlex.split(requirement["command"])])
    for groups in hooks["hooks"].values():
        groups[0]["hooks"][0]["command"] = command
    source.write_text(json.dumps(hooks))
    requirement["command"] = command
    requirement["source_sha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
    os.environ["CODEX_DEMONCODER_COMPACTION_RELAY"] = json.dumps(requirement)
os.environ["CODEX_HOME"] = str(root / "codex-home")
os.environ["HOME"] = str(root / "home")
for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"]:
    os.environ[key] = config["endpoint"]
os.environ["NO_PROXY"] = ""
os.environ["CODEX_CA_CERTIFICATE"] = config["ca"]
with (root / "backend-stderr.txt").open("wb") as errors, (root / "backend-wire.jsonl").open("wb") as wire:
    child = subprocess.Popen([config["binary"], *sys.argv[1:]], stdin=sys.stdin, stdout=subprocess.PIPE, stderr=errors)
    (root / "backend.pid").write_text(str(child.pid))
    for line in child.stdout:
        wire.write(line)
        wire.flush()
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
    raise SystemExit(child.wait())
