"""Adversarial backend peer: queue tools before and after interruption."""
import json
from pathlib import Path
import sys


def initial_calls(token):
    return [
        {"id": "held", "name": "bash", "arguments": {"command": f"printf 'STEER-WAIT-{token}\\n'; while ! test -f release-tool; do sleep 0.01; done; printf 'HELD-DONE\\n'"}},
        {"id": "superseded", "name": "write", "arguments": {"path": "superseded.txt", "content": "must not execute"}},
    ]


def corrected_call(correction):
    return {"id": "corrected", "name": "write", "arguments": {"path": "corrected.txt", "content": correction}}


def run(codex):
    def send(value):
        print(json.dumps(value), flush=True)

    turn = 0
    completed_result = None
    denied = set()
    interrupt = None
    correction = None

    def call(value):
        if codex:
            send({"id": value["id"], "method": "item/tool/call", "params": {"threadId": "fixture-thread", "turnId": f"turn-{turn}", "callId": value["id"], "tool": value["name"], "arguments": value["arguments"]}})
        else:
            send({"type": "control_request", "request_id": value["id"], "request": {"subtype": "mcp_message", "server_name": "demoncoder", "message": {"jsonrpc": "2.0", "id": value["id"], "method": "tools/call", "params": {"name": value["name"], "arguments": value["arguments"]}}}})

    def finish(interrupted):
        if codex:
            if not interrupted:
                send({"method": "item/agentMessage/delta", "params": {"threadId": "fixture-thread", "turnId": f"turn-{turn}", "delta": "STEER-APPLIED"}})
            send({"method": "turn/completed", "params": {"threadId": "fixture-thread", "turn": {"id": f"turn-{turn}", "status": "interrupted" if interrupted else "completed"}}})
        else:
            if not interrupted:
                send({"type": "stream_event", "session_id": "fixture-session", "event": {"delta": {"type": "text_delta", "text": "STEER-APPLIED"}}})
            send({"type": "result", "session_id": "fixture-session", "subtype": "error_during_execution" if interrupted else "success", "is_error": interrupted})

    for raw in sys.stdin:
        message = json.loads(raw)
        is_result = (codex and "result" in message) or (not codex and message.get("type") == "control_response")
        if is_result:
            identifier = message["id"] if codex else message["response"]["request_id"]
            result = message["result"] if codex else message["response"]["response"]["mcp_response"]["result"]
            success = result["success"] if codex else not result["isError"]
            if identifier in ("superseded", "late"):
                assert not success, "superseded tool was admitted"
                denied.add(identifier)
            else:
                actual = json.loads(result["contentItems"][0]["text"] if codex else result["content"][0]["text"])
                assert actual["success"]
                if identifier == "held":
                    assert "HELD-DONE" in actual["output"] and actual["exit_code"] == 0
                    completed_result = actual
                elif identifier == "corrected":
                    finish(False)
            if interrupt is not None and denied == {"superseded", "late"}:
                # Deliberately deliver completion before the interrupt acknowledgement.
                finish(True)
                if codex:
                    send({"id": interrupt, "result": {}})
                else:
                    send({"type": "control_response", "response": {"subtype": "success", "request_id": interrupt, "response": {}}})
                interrupt = None
            continue
        method = message.get("method") if codex else message.get("request", {}).get("subtype")
        if method in ("turn/interrupt", "interrupt"):
            assert completed_result, "interrupted before returning completed tool evidence"
            if codex:
                assert message["params"] == {"threadId": "fixture-thread", "turnId": "turn-1"}
            interrupt = message["id"] if codex else message["request_id"]
            late = initial_calls("unused")[1]
            late["id"] = "late"
            call(late)
        elif method == "initialize":
            if codex:
                send({"id": message["id"], "result": {"userAgent": "fixture"}})
            else:
                send({"type": "control_response", "response": {"subtype": "success", "request_id": message["request_id"], "response": {}}})
        elif method == "account/read":
            send({"id": message["id"], "result": {"requiresOpenaiAuth": True, "account": {"type": "chatgpt"}}})
        elif method == "config/read":
            send({"id": message["id"], "result": {"config": {"mcp_servers": {}}}})
        elif method == "thread/start":
            send({"id": message["id"], "result": {"thread": {"id": "fixture-thread"}}})
        elif method == "turn/start" or message.get("type") == "user":
            turn += 1
            if codex:
                assert message["params"]["threadId"] == "fixture-thread"
                prompt = message["params"]["input"][0]["text"]
                send({"id": message["id"], "result": {"turn": {"id": f"turn-{turn}"}}})
            else:
                assert turn == 1 or message["session_id"] == "fixture-session"
                prompt = message["message"]["content"]
                send({"type": "system", "subtype": "init", "session_id": "fixture-session", "apiKeySource": "none"})
            if turn == 1:
                for value in initial_calls(prompt):
                    call(value)
            else:
                assert turn == 2 and completed_result and denied == {"superseded", "late"}
                correction = prompt
                assert correction.startswith("CORRECT-")
                Path("steering-audit.json").write_text(json.dumps({"correction": correction, "completed_result": completed_result, "denied": sorted(denied)}))
                call(corrected_call(correction))
