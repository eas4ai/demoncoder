#!/usr/bin/python3
"""Paused usage records for the real API and subprocess adapter parsers."""
import json
from pathlib import Path
import sys
import time

ADAPTERS = ("openai-api", "anthropic-api", "codex", "claude")


def reported(adapter, mode):
    offset = ADAPTERS.index(adapter)
    if mode == "known":
        return {"input_tokens": 11 + offset, "output_tokens": 7 + offset, "cache_read_input_tokens": 2 + offset, "cache_creation_input_tokens":5}
    if mode == "partial":
        return {"input_tokens": 0}
    if mode == "zero":
        return {"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens":0}
    assert mode == "absent"
    return {}


def send(value):
    print(json.dumps(value), flush=True)


def run():
    codex = "app-server" in sys.argv
    adapter = "codex" if codex else "claude"
    identity = "usage-thread" if codex else "usage-session"
    for raw in sys.stdin:
        message = json.loads(raw)
        method = message.get("method")
        if codex and method in ("initialize", "account/read", "config/read", "thread/start"):
            result = {"initialize":{"userAgent":"usage-fixture"}, "account/read":{"account":{"type":"chatgpt"}}, "config/read":{"config":{"mcp_servers":{}}}, "thread/start":{"thread":{"id":identity}}}[method]
            send({"id":message["id"], "result":result})
        elif not codex and message.get("type") == "control_request":
            send({"type":"control_response", "response":{"subtype":"success", "request_id":message["request_id"], "response":{}}})
        elif method == "turn/start" or message.get("type") == "user":
            mode = message["params"]["input"][0]["text"] if codex else message["message"]["content"]
            usage = reported(adapter, mode)
            turn = "usage-" + mode
            if codex:
                send({"id":message["id"], "result":{"turn":{"id":turn}}})
                send({"method":"item/agentMessage/delta", "params":{"threadId":identity, "turnId":turn, "delta":"USAGE-WAIT-" + mode}})
            else:
                send({"type":"system", "subtype":"init", "session_id":identity, "apiKeySource":"none"})
                send({"type":"stream_event", "session_id":identity, "event":{"delta":{"type":"text_delta", "text":"USAGE-WAIT-" + mode}}})
            deadline = time.monotonic() + 15
            while not Path("release-" + mode).exists():
                assert time.monotonic() < deadline, "usage checkpoint was not released"
                time.sleep(.01)
            if codex:
                if mode != "absent":
                    names = {"input_tokens":"inputTokens", "output_tokens":"outputTokens", "cache_read_input_tokens":"cachedInputTokens"}
                    send({"method":"thread/tokenUsage/updated", "params":{"threadId":identity, "turnId":turn, "tokenUsage":{"last":{names[k]:v for k,v in usage.items() if k in names}, "total":{"totalTokens":999999}, "modelContextWindow":20000}}})
                send({"method":"turn/completed", "params":{"threadId":identity, "turn":{"id":turn, "status":"completed"}}})
            else:
                send({"type":"stream_event", "session_id":identity, "event":{"type":"message_start", "message":{"usage":{k:v for k,v in usage.items() if k != "output_tokens"}}}})
                send({"type":"stream_event", "session_id":identity, "event":{"type":"message_delta", "usage":{k:v for k,v in usage.items() if k == "output_tokens"}}})
                # Per-request assistant usage is independent of aggregate result usage.
                send({"type":"assistant", "session_id":identity, "message":{"usage":usage}})
                result = {"type":"result", "subtype":"success", "is_error":False, "session_id":identity, "usage":usage}
                if mode == "known":
                    result["usage"] = {**usage, "input_tokens":usage["input_tokens"] + 90000}
                if mode in ("known", "zero"):
                    result["total_cost_usd"] = .0123 if mode == "known" else 0
                send(result)


if __name__ == "__main__":
    run()
