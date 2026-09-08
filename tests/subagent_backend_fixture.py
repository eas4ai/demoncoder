#!/usr/bin/python3
"""Controlled subscription peer; coding effects only occur through host tool calls."""
import json
import os
from pathlib import Path
import sys


def send(value):
    print(json.dumps(value), flush=True)


def assignment(prompt):
    return json.loads(prompt.split("\nAssignment: ", 1)[1].split("\nSelected checks:", 1)[0])


def calls_for(request):
    if request["context"]:
        return json.loads(request["context"])["fixture_calls"]
    return [
        {"name": "read", "arguments": {"path": "greeting"}},
        {"name": "write", "arguments": {"path": "greeting", "content": "child draft\n"}},
        {"name": "edit", "arguments": {"path": "greeting", "old_text": "child draft", "new_text": "child result"}},
        {"name": "bash", "arguments": {"command": 'test "$(cat greeting)" = "child result"'}},
    ]


def main():
    codex = "app-server" in sys.argv
    assert "OPENAI_API_KEY" not in os.environ
    assert "ANTHROPIC_API_KEY" not in os.environ
    pending = []
    index = 0
    turn = "fixture-turn"

    def next_call():
        nonlocal index
        if index == len(pending):
            if codex:
                send({"method": "item/agentMessage/delta", "params": {"threadId": "fixture-thread", "turnId": turn, "delta": "Child finished; inspect original results."}})
                send({"method": "turn/completed", "params": {"threadId": "fixture-thread", "turn": {"id": turn, "status": "completed"}}})
            else:
                send({"type": "stream_event", "session_id": "fixture-session", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Child finished; inspect original results."}}})
                send({"type": "result", "subtype": "success", "is_error": False, "session_id": "fixture-session", "usage": {"input_tokens": 12, "output_tokens": 8}})
            return
        call = pending[index]
        index += 1
        identifier = f"child-{index}"
        if codex:
            send({"id": identifier, "method": "item/tool/call", "params": {"threadId": "fixture-thread", "turnId": turn, "callId": identifier, "tool": call["name"], "arguments": call["arguments"]}})
        else:
            send({"type": "control_request", "request_id": identifier, "request": {"subtype": "mcp_message", "server_name": "demoncoder", "message": {"jsonrpc": "2.0", "id": identifier, "method": "tools/call", "params": call}}})

    for line in sys.stdin:
        message = json.loads(line)
        if codex:
            method = message.get("method")
            result = None
            if method == "initialize":
                result = {"userAgent": "subagent-fixture"}
            elif method == "config/read":
                result = {"config": {"mcp_servers": {}}}
            elif method == "account/read":
                result = {"requiresOpenaiAuth": True, "account": {"type": "chatgpt", "email": "fixture@example.invalid", "planType": "plus"}}
            elif method == "thread/start":
                assert message["params"]["sandbox"] == "workspace-write"
                assert {tool["name"] for tool in message["params"]["dynamicTools"]} == {"read", "write", "edit", "bash"}
                result = {"thread": {"id": "fixture-thread"}}
            elif method == "turn/start":
                request = assignment(message["params"]["input"][0]["text"])
                pending = calls_for(request)
                index = 0
                send({"id": message["id"], "result": {"turn": {"id": turn, "status": "inProgress"}}})
                send({"method": "turn/started", "params": {"threadId": "fixture-thread", "turn": {"id": turn}}})
                next_call()
            elif isinstance(message.get("id"), str) and message["id"].startswith("child-"):
                next_call()
            if result is not None:
                send({"id": message["id"], "result": result})
        elif message.get("type") == "control_request":
            assert message["request"]["subtype"] == "initialize"
            assert message["request"]["hooks"] is None
            send({"type": "control_response", "response": {"subtype": "success", "request_id": message["request_id"], "response": {}}})
        elif message.get("type") == "user":
            assert sys.argv[sys.argv.index("--tools") + 1] == ""
            pending = calls_for(assignment(message["message"]["content"]))
            index = 0
            send({"type": "system", "subtype": "init", "session_id": "fixture-session", "apiKeySource": "none"})
            next_call()
        elif message.get("type") == "control_response":
            next_call()


if __name__ == "__main__":
    main()
