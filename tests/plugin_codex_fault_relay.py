#!/usr/bin/python3
"""Inject a wire fault around the real host relay for actual backend qualification."""
import json
from pathlib import Path
import socket
import subprocess
import sys

mode = sys.argv[1]
command = sys.argv[2:]
request = json.load(sys.stdin)
target = "PostCompact" if mode.startswith("post-") else "PreCompact"
fault = mode.removeprefix("post-")
selected = request["hook_event_name"] == target
if selected:
    Path("relay-fault.json").write_text(json.dumps(request))
if selected and fault in ["forgery", "disconnect"]:
    address = json.loads(Path(command[-1]).read_text())
    with socket.socket(socket.AF_UNIX) as peer:
        peer.connect(address["socket"])
        if fault == "forgery":
            peer.sendall(json.dumps({"token":"forged", "input":request}).encode())
            peer.shutdown(socket.SHUT_WR)
            while peer.recv(4096):
                pass
    raise SystemExit(1)
if selected and fault == "wrong-turn":
    request["turn_id"] = "different-turn"
response = subprocess.run(command, input=json.dumps(request).encode(), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=66)
if response.returncode != 0:
    raise SystemExit(response.returncode)
if selected and fault == "retry":
    repeated = subprocess.run(command, input=json.dumps(request).encode(), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=66)
    assert repeated.returncode == 0 and repeated.stdout == response.stdout, "retry changed its admitted decision"
if selected and fault == "duplicate":
    sys.stdout.buffer.write(response.stdout + response.stdout)
else:
    sys.stdout.buffer.write(response.stdout)
