import json
import os
import subprocess
import sys
import time
import signal
import ctypes

calls = 0
lists = 0
child = subprocess.Popen(["/usr/bin/python3", "-c", "import time; time.sleep(120)", sys.argv[1]], start_new_session=True)
if "idle-ping" in sys.argv[1]:
    signal.signal(signal.SIGUSR1, lambda *_: print(json.dumps({"jsonrpc": "2.0", "id": 987654, "method": "ping"}), flush=True))
for line in sys.stdin:
    request = json.loads(line)
    if "idle-ping" in sys.argv[1] and request.get("id") == 987654 and "method" not in request:
        assert request.get("result") == {}
        # The test observes actual receipt of the host reply through this process name.
        assert ctypes.CDLL(None).prctl(15, b"mcp-ping-reply", 0, 0, 0) == 0
        continue
    if "id" not in request:
        continue
    method = request["method"]
    if method == "initialize":
        if "stall-start" in sys.argv[1]:
            time.sleep(120)
        result = {"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "fixture", "version": "1"}}
    elif method == "tools/list":
        lists += 1
        result = {"tools": [{"name": "gate", "inputSchema": {"type": "object", "additionalProperties": True}}]}
    elif method == "tools/call":
        calls += 1
        event = request["params"]["arguments"]["event"]
        if event == "SessionEnd" and "stall-end" in sys.argv[1]:
            time.sleep(120)
        assert (calls, lists, event) in [(1, 1, "SessionStart"), (2, 2, "SessionEnd")]
        result = {"content": [], "structuredContent": {}}
    else:
        raise RuntimeError("unexpected method")
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
