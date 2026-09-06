#!/usr/bin/python3
"""Synthetic subscription login outcomes; never contacts a provider."""
import json
import os
from pathlib import Path
import sys


def send(value):
    print(json.dumps(value), flush=True)


def main():
    codex = "app-server" in sys.argv
    mode = Path("auth-mode").read_text()
    assert "OPENAI_API_KEY" not in os.environ
    assert "ANTHROPIC_API_KEY" not in os.environ
    assert str(Path.cwd()) == os.environ["HOME"]
    if codex:
        assert 'forced_login_method="chatgpt"' in sys.argv
    else:
        assert sys.argv[sys.argv.index("--setting-sources") + 1] == ""
    for raw in sys.stdin:
        message = json.loads(raw)
        with Path("auth-messages.jsonl").open("a") as log:
            log.write(json.dumps(message) + "\n")
        method = message.get("method")
        if codex:
            if method == "initialize":
                send({"id":message["id"], "result":{"userAgent":"auth-fixture"}})
            elif method == "account/read":
                if mode == "expired":
                    send({"id":message["id"], "error":{"code":401,"message":"subscription login expired"}})
                else:
                    account = None if mode == "missing" else {"type":"apiKey" if mode == "wrong-route" else "chatgpt"}
                    send({"id":message["id"], "result":{"account":account}})
            elif method == "config/read":
                send({"id":message["id"], "result":{"config":{"mcp_servers":{}}}})
            elif method == "thread/start":
                send({"id":message["id"], "result":{"thread":{"id":"auth-thread"}}})
            elif method == "turn/start":
                send({"id":message["id"], "result":{"turn":{"id":"auth-turn", "status":"inProgress"}}})
                send({"method":"item/agentMessage/delta", "params":{"threadId":"auth-thread", "turnId":"auth-turn", "delta":"AUTH-OK"}})
                send({"method":"turn/completed", "params":{"threadId":"auth-thread", "turn":{"id":"auth-turn", "status":"completed"}}})
        elif message.get("type") == "control_request":
            send({"type":"control_response", "response":{"subtype":"success", "request_id":message["request_id"], "response":{}}})
        elif message.get("type") == "user":
            if mode in ("missing", "expired"):
                send({"type":"result", "subtype":"error_during_execution", "is_error":True, "session_id":"auth-session", "errors":["subscription login " + mode]})
                continue
            init = {"type":"system", "subtype":"init", "session_id":"auth-session"}
            if mode != "unknown-route":
                init["apiKeySource"] = "user" if mode == "wrong-route" else "none"
            if mode == "tool-before-init":
                send({"type":"control_request", "request_id":"unauth-tool", "request":{"subtype":"mcp_message", "server_name":"demoncoder", "message":{"jsonrpc":"2.0", "id":"unauth-tool", "method":"tools/call", "params":{"name":"write", "arguments":{"path":"unauth-effect", "content":"forbidden"}}}}})
            if mode != "no-init":
                send(init)
            send({"type":"stream_event", "session_id":"auth-session", "event":{"type":"content_block_delta", "delta":{"type":"text_delta", "text":"AUTH-OK"}}})
            send({"type":"result", "subtype":"success", "is_error":False, "session_id":"auth-session"})


if __name__ == "__main__":
    main()
