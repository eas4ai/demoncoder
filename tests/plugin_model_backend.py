#!/usr/bin/python3
"""Controlled backend protocol peer for bounded model-hook transport tests."""

import json
import os
from pathlib import Path
import sys
import time

config = json.loads(Path(__file__).with_suffix(".json").read_text())


def send(value):
    print(json.dumps(value), flush=True)


def record(value):
    with open(config["records"], "a") as output:
        output.write(json.dumps(value) + "\n")


record(
    {
        "argv": sys.argv[1:],
        "cwd": str(Path.cwd()),
        "pid": os.getpid(),
        "mode": os.environ.get("CODEX_DEMONCODER_MODEL_HOOK"),
    }
)
if "--demoncoder-model-hook-capability" in sys.argv:
    send({"protocol": "demoncoder-model-hook-v1", "source_version": "0.153.4"})
    sys.exit(0)
codex = "app-server" in sys.argv
assert str(Path.cwd()) != config["workspace"]
assert not Path("public").exists()
assert "OPENAI_API_KEY" not in os.environ
assert "ANTHROPIC_API_KEY" not in os.environ
if codex:
    assert os.environ["CODEX_DEMONCODER_MODEL_HOOK"] == "v1"
else:
    assert "--safe-mode" in sys.argv
    assert sys.argv[sys.argv.index("--model") + 1] == "explicit-hook-model"
    assert sys.argv[sys.argv.index("--tools") + 1] == ""
expected = (
    ["snapshot_read", "snapshot_list", "snapshot_search"] if config["agent"] else []
)


def complete():
    text = json.dumps({"ok": True})
    if codex:
        send(
            {
                "method": "thread/tokenUsage/updated",
                "params": {
                    "threadId": "thread",
                    "turnId": "turn",
                    "tokenUsage": {
                        "last": {
                            "inputTokens": 13,
                            "outputTokens": 9,
                            "cachedInputTokens": 3,
                        }
                    },
                },
            }
        )
        send(
            {
                "method": "item/agentMessage/delta",
                "params": {"threadId": "thread", "turnId": "turn", "delta": text},
            }
        )
        send(
            {
                "method": "turn/completed",
                "params": {
                    "threadId": "thread",
                    "turn": {"id": "turn", "status": "completed"},
                },
            }
        )
    else:
        send(
            {
                "type": "stream_event",
                "session_id": "session",
                "event": {
                    "type": "content_block_delta",
                    "delta": {"type": "text_delta", "text": text},
                },
            }
        )
        send(
            {
                "type": "result",
                "subtype": "success",
                "is_error": False,
                "session_id": "session",
                "usage": {
                    "input_tokens": 13,
                    "output_tokens": 9,
                    "cache_read_input_tokens": 3,
                },
            }
        )


def start(prompt):
    assert "retained source" in prompt
    assert "literal $(touch /tmp/not-executed) `text` $ARGUMENTS" in prompt
    if config["behavior"] == "timeout":
        time.sleep(30)
    if config["agent"] or config["behavior"] == "forbidden":
        name = "bash" if config["behavior"] == "forbidden" else "snapshot_read"
        arguments = (
            {"command": "touch forbidden"}
            if name == "bash"
            else {"path": config["workspace"] + "/public"}
        )
        if codex:
            send(
                {
                    "id": "inspection",
                    "method": "item/tool/call",
                    "params": {
                        "threadId": "thread",
                        "turnId": "turn",
                        "callId": "inspection",
                        "tool": name,
                        "arguments": arguments,
                    },
                }
            )
        else:
            send(
                {
                    "type": "control_request",
                    "request_id": "inspection",
                    "request": {
                        "subtype": "mcp_message",
                        "server_name": "demoncoder",
                        "message": {
                            "jsonrpc": "2.0",
                            "id": "inspection",
                            "method": "tools/call",
                            "params": {"name": name, "arguments": arguments},
                        },
                    },
                }
            )
    else:
        complete()


for raw in sys.stdin:
    message = json.loads(raw)
    record({"message": message})
    if codex:
        method = message.get("method")
        result = None
        if method == "initialize":
            result = {"userAgent": "model-hook-fixture"}
        elif method == "account/read":
            result = {"account": {"type": "chatgpt"}, "requiresOpenaiAuth": True}
        elif method == "config/read":
            result = {
                "config": {
                    "mcp_servers": {},
                    "model_provider": "openai",
                    "model_providers": {},
                }
            }
        elif method == "thread/start":
            params = message["params"]
            assert params["model"] == "explicit-hook-model"
            assert [tool["name"] for tool in params["dynamicTools"]] == expected
            assert "read-only lifecycle gate" in params["developerInstructions"]
            result = {"thread": {"id": "thread"}}
        elif method == "turn/start":
            send(
                {
                    "id": message["id"],
                    "result": {"turn": {"id": "turn", "status": "inProgress"}},
                }
            )
            send(
                {
                    "method": "turn/started",
                    "params": {"threadId": "thread", "turn": {"id": "turn"}},
                }
            )
            start(message["params"]["input"][0]["text"])
        elif message.get("id") == "inspection" and "result" in message:
            result_value = json.loads(message["result"]["contentItems"][0]["text"])
            if config["behavior"] != "forbidden":
                assert (
                    result_value["success"]
                    and "retained source" in result_value["output"]
                    and "revision" in result_value["output"]
                )
            complete()
        if result is not None:
            send({"id": message["id"], "result": result})
    elif (
        message.get("type") == "control_request"
        and message["request"]["subtype"] == "initialize"
    ):
        send(
            {
                "type": "control_response",
                "response": {
                    "subtype": "success",
                    "request_id": message["request_id"],
                    "response": {},
                },
            }
        )
        send(
            {
                "type": "control_request",
                "request_id": "list",
                "request": {
                    "subtype": "mcp_message",
                    "server_name": "demoncoder",
                    "message": {
                        "jsonrpc": "2.0",
                        "id": "list",
                        "method": "tools/list",
                        "params": {},
                    },
                },
            }
        )
    elif message.get("type") == "control_response":
        response = message["response"]
        if response["request_id"] == "list":
            assert [
                tool["name"]
                for tool in response["response"]["mcp_response"]["result"]["tools"]
            ] == expected
        elif response["request_id"] == "inspection":
            result_value = json.loads(
                response["response"]["mcp_response"]["result"]["content"][0]["text"]
            )
            if config["behavior"] != "forbidden":
                assert (
                    result_value["success"]
                    and "retained source" in result_value["output"]
                    and "revision" in result_value["output"]
                )
            complete()
    elif message.get("type") == "user":
        send(
            {
                "type": "system",
                "subtype": "init",
                "session_id": "session",
                "apiKeySource": "none",
            }
        )
        start(message["message"]["content"])
