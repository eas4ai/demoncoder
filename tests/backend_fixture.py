#!/usr/bin/python3
"""Protocol peers for the production CLI adapters; never launch a real model."""
import json
import os
import shlex
import subprocess
from pathlib import Path
import sys
import time
sys.dont_write_bytecode = True
from tool_cycle_fixture import Cycle


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
    if Path("continuation").exists():
        from continuation_fixture import run
        return run(codex)
    if Path("steering").exists():
        from steering_fixture import run
        return run(codex)
    expected = json.loads(Path("settings-expect.json").read_text()) if Path("settings-expect.json").exists() else None
    if expected:
        variable, suffix = ("CODEX_HOME", ".codex-selected") if codex else ("CLAUDE_CONFIG_DIR", ".claude-selected")
        assert os.environ[variable] == str(Path.cwd() / suffix)
    if expected and not codex:
        assert sys.argv[sys.argv.index("--model") + 1] == expected["model"]
        assert sys.argv[sys.argv.index("--effort") + 1] == expected["effort"]
    turns = 0
    cycle = None
    response = None
    turn = None
    def request_call(call):
        if Path("host-access").exists() and call["name"] == "bash":
            call["arguments"]["command"] = 'set -e; test "$PWD" = ' + shlex.quote(str(Path.cwd())) + "; " + call["arguments"]["command"]
        if codex:
            send({"id": "tool-" + call["id"], "method": "item/tool/call", "params": {"threadId": "fixture-thread", "turnId": turn, "callId": call["id"], "tool": call["name"], "arguments": call["arguments"]}})
        else:
            send({"type": "control_request", "request_id": call["id"], "request": {"subtype": "mcp_message", "server_name": "demoncoder", "message": {"jsonrpc": "2.0", "id": call["id"], "method": "tools/call", "params": {"name": call["name"], "arguments": call["arguments"]}}}})

    def complete():
        if codex:
            send({"method": "item/agentMessage/delta", "params": {"threadId": "fixture-thread", "turnId": turn, "delta": response}})
            send({"method": "turn/completed", "params": {"threadId": "fixture-thread", "turn": {"id": turn, "status": "completed"}}})
        else:
            send({"type": "stream_event", "session_id": "fixture-session", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": response}}})
            send({"type": "result", "subtype": "success", "is_error": False, "session_id": "fixture-session", "usage": {"input_tokens": 12, "output_tokens": 8}})

    def start_responsive(token):
        nonlocal response
        def wait_release():
            deadline = time.monotonic() + 10
            while not Path("release-provider").exists():
                assert time.monotonic() < deadline, "provider release timed out"
                time.sleep(0.01)
        if Path("buffer-assistant").exists(): wait_release()
        if codex:
            send({"method":"item/agentMessage/delta", "params":{"threadId":"fixture-thread", "turnId":turn, "delta":"ASSISTANT-WAIT-" + token}})
        else:
            send({"type":"stream_event", "session_id":"fixture-session", "event":{"type":"content_block_delta", "delta":{"type":"text_delta", "text":"ASSISTANT-WAIT-" + token}}})
        wait_release()
        response = "SESSION-DONE"
        request_call({"id":token + "-bash", "name":"bash", "arguments":{"command": f"printf 'TOOL-WAIT-{token}\\n'; while ! test -f release-tool; do sleep 0.01; done; printf 'TOOL-DONE\\n'"}})

    def start_cancellation(prompt):
        nonlocal response
        if prompt.startswith("NEXT-"):
            response = "READY-" + prompt
            complete()
        elif Path("cancellation").read_text() == "tool":
            from cancellation import held_tool
            request_call(held_tool(prompt))
        else:
            # A real backend helper must stop along with its parent.
            subprocess.Popen(["/usr/bin/python3", "-u", "-c",
                "import time; f=open('heartbeat','a');\nwhile True: f.write('x'); f.flush(); time.sleep(.02)"])
            if codex:
                send({"method":"item/agentMessage/delta", "params":{"threadId":"fixture-thread", "turnId":turn, "delta":"CANCELWAIT-" + prompt}})
            else:
                send({"type":"stream_event", "session_id":"fixture-session", "event":{"type":"content_block_delta", "delta":{"type":"text_delta", "text":"CANCELWAIT-" + prompt}}})

    for raw in sys.stdin:
        message = json.loads(raw)
        if Path("responsiveness").exists() and ((codex and "result" in message) or (not codex and message.get("type") == "control_response")):
            result = json.loads(message["result"]["contentItems"][0]["text"] if codex else message["response"]["response"]["mcp_response"]["result"]["content"][0]["text"])
            assert result["success"] and "TOOL-DONE" in result["output"]
            complete()
            continue
        if cycle and ((codex and "result" in message) or (not codex and message.get("type") == "control_response")):
            result = json.loads(message["result"]["contentItems"][0]["text"] if codex else message["response"]["response"]["mcp_response"]["result"]["content"][0]["text"])
            call = cycle.next(result)
            if call: request_call(call)
            else:
                cycle = None
                complete()
            continue
        if codex:
            method = message.get("method")
            result = None
            if method == "initialize":
                result = {"userAgent": "test-fixture"}
            elif method == "config/read":
                result = {"config": {"mcp_servers": {}}}
            elif method == "account/read":
                result = {"account": {"type": "chatgpt", "email": "fixture@example.invalid", "planType": "plus"}}
            elif method in ("thread/start", "thread/resume"):
                if expected: assert message["params"]["model"] == expected["model"]
                assert message["params"]["sandbox"] == ("danger-full-access" if Path("host-access").exists() else "workspace-write")
                assert message["params"]["approvalPolicy"] == "never"
                if method == "thread/start":
                    assert message["params"]["environments"] == []
                    assert {tool["name"] for tool in message["params"]["dynamicTools"]} == {"read", "write", "edit", "bash"}
                else:
                    assert message["params"]["threadId"] == "fixture-thread"
                result = {"thread": {"id": "fixture-thread"}}
            elif method == "turn/start":
                if expected: assert message["params"]["effort"] == expected["effort"]
                assert message["params"]["environments"] == []
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
                if Path("cancellation").exists(): start_cancellation(prompt)
                elif Path("responsiveness").exists(): start_responsive(prompt)
                elif Path("tool-cycle").exists():
                    cycle = Cycle(prompt, Path("wrong-edit").exists())
                    request_call(cycle.next())
                else: complete()
            if result is not None:
                send({"id": message["id"], "result": result})
        elif message.get("type") == "control_request" and message["request"]["subtype"] == "initialize":
            send({"type": "control_response", "response": {"subtype": "success", "request_id": message["request_id"], "response": {}}})
        elif message.get("type") == "user":
            prompt = message["message"]["content"]
            response = record(prompt)
            send({"type": "system", "subtype": "init", "session_id": "fixture-session", "apiKeySource": "none"})
            if Path("cancellation").exists(): start_cancellation(prompt)
            elif Path("responsiveness").exists(): start_responsive(prompt)
            elif Path("tool-cycle").exists():
                cycle = Cycle(prompt, Path("wrong-edit").exists())
                request_call(cycle.next())
            else: complete()


if __name__ == "__main__":
    main()
