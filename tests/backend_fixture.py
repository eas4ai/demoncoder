#!/usr/bin/python3
"""Protocol peers for the production CLI adapters; never launch a real model."""
import json
import os
from pathlib import Path
import sys


def send(value):
    print(json.dumps(value), flush=True)


def record(prompt):
    # The fixture's cwd is the same temporary workspace passed to the application.
    Path("received-prompt.json").write_text(json.dumps(prompt))
    return "RECEIVED-" + prompt


def main():
    codex = "app-server" in sys.argv
    assert "OPENAI_API_KEY" not in os.environ
    assert "ANTHROPIC_API_KEY" not in os.environ
    turns = 0
    for raw in sys.stdin:
        message = json.loads(raw)
        if codex:
            method = message.get("method")
            result = None
            if method == "initialize":
                result = {"userAgent": "test-fixture"}
            elif method == "account/read":
                result = {"account": {"type": "chatgpt", "email": "fixture@example.invalid", "planType": "plus"}}
            elif method == "thread/start":
                assert message["params"]["sandbox"] == "read-only"
                assert message["params"]["approvalPolicy"] == "never"
                result = {"thread": {"id": "fixture-thread"}}
            elif method == "turn/start":
                assert message["params"]["threadId"] == "fixture-thread"
                prompt = message["params"]["input"][0]["text"]
                if Path("drop-prompt").exists():
                    send({"id": message["id"], "error": {"code": -1, "message": "fixture refuses to start"}})
                    continue
                turns += 1
                turn = "fixture-turn-" + str(turns)
                response = record(prompt)
                send({"id": message["id"], "result": {"turn": {"id": turn, "status": "inProgress"}}})
                send({"method": "turn/started", "params": {"threadId": "fixture-thread", "turn": {"id": turn}}})
                send({"method": "item/agentMessage/delta", "params": {"threadId": "fixture-thread", "turnId": turn, "delta": response}})
                send({"method": "turn/completed", "params": {"threadId": "fixture-thread", "turn": {"id": turn, "status": "completed"}}})
            if result is not None:
                send({"id": message["id"], "result": result})
        elif message.get("type") == "user":
            prompt = message["message"]["content"]
            response = record(prompt)
            send({"type": "system", "subtype": "init", "session_id": "fixture-session", "apiKeySource": "none"})
            send({"type": "stream_event", "session_id": "fixture-session", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": response}}})
            send({"type": "result", "subtype": "success", "is_error": False, "session_id": "fixture-session", "usage": {"input_tokens": 12, "output_tokens": 8}})


if __name__ == "__main__":
    main()
