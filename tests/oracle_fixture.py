#!/usr/bin/python3
"""No-tools subscription peers for Oracle transport and pending-review tests."""
import json
import os
from pathlib import Path
import sys
import time


def send(value):
    print(json.dumps(value), flush=True)


def main():
    codex = "app-server" in sys.argv
    assert "OPENAI_API_KEY" not in os.environ
    assert "ANTHROPIC_API_KEY" not in os.environ
    if not codex:
        assert sys.argv[sys.argv.index("--tools") + 1] == ""
        assert sys.argv[sys.argv.index("--setting-sources") + 1] == ""
    for raw in sys.stdin:
        message = json.loads(raw)
        method = message.get("method")
        if codex and method in ("initialize", "config/read", "account/read", "thread/start"):
            result = {}
            if method == "config/read":
                result = {"config": {"mcp_servers": {}}}
            if method == "account/read":
                result = {"account": {"type": "chatgpt", "email": "oracle@example.invalid", "planType": "plus"}}
            if method == "thread/start":
                assert message["params"]["dynamicTools"] == []
                assert message["params"]["environments"] == []
                result = {"thread": {"id": "oracle-thread"}}
            send({"id": message["id"], "result": result})
        elif not codex and message.get("type") == "control_request":
            send({"type": "control_response", "response": {"subtype": "success", "request_id": message["request_id"], "response": {}}})
        elif (codex and method == "turn/start") or message.get("type") == "user":
            prompt = message["params"]["input"][0]["text"] if codex else message["message"]["content"]
            request = json.loads(prompt.strip().splitlines()[-1])
            Path("oracle-request.json").write_text(json.dumps(request))
            Path("oracle-pid").write_text(str(os.getpid()))
            mode = Path("oracle-mode").read_text()
            if codex:
                send({"id": message["id"], "result": {"turn": {"id": "oracle-turn", "status": "inProgress"}}})
            else:
                send({"type": "system", "subtype": "init", "session_id": "oracle-session", "apiKeySource": "none"})
                send({"type": "control_request", "request_id": "catalog", "request": {"subtype": "mcp_message", "server_name": "demoncoder", "message": {"jsonrpc": "2.0", "id": "catalog", "method": "tools/list"}}})
                reply = json.loads(next(sys.stdin))
                assert reply["response"]["response"]["mcp_response"]["result"]["tools"] == []
            if mode == "hold":
                while not Path("oracle-release").exists():
                    time.sleep(.01)
                mode = "allow"
            if mode == "tool":
                if codex:
                    send({"id":"illegal", "method":"item/tool/call", "params":{"threadId":"oracle-thread", "turnId":"oracle-turn", "callId":"illegal", "tool":"write", "arguments":{"path":"oracle-effect", "content":"forbidden"}}})
                else:
                    send({"type":"control_request", "request_id":"illegal", "request":{"subtype":"mcp_message", "server_name":"demoncoder", "message":{"jsonrpc":"2.0", "id":"illegal", "method":"tools/call", "params":{"name":"write", "arguments":{"path":"oracle-effect", "content":"forbidden"}}}}})
            text = "invalid decision" if mode == "invalid" else json.dumps({"decision": "deny" if mode == "deny" else "allow", "reason": "Fixture verdict after checking the empty tool catalog."})
            if codex:
                send({"method":"item/agentMessage/delta", "params":{"threadId":"oracle-thread", "turnId":"oracle-turn", "delta":text}})
                send({"method":"turn/completed", "params":{"threadId":"oracle-thread", "turn":{"id":"oracle-turn", "status":"completed"}}})
            else:
                send({"type":"stream_event", "session_id":"oracle-session", "event":{"type":"content_block_delta", "delta":{"type":"text_delta", "text":text}}})
                send({"type":"result", "subtype":"success", "is_error":False, "session_id":"oracle-session", "usage":{"input_tokens":12,"output_tokens":8}})


if __name__ == "__main__":
    main()
