"""Two-turn coding peer with backend-owned persisted fixture state."""
import json
from pathlib import Path
import sys
import uuid

SECOND_PROMPT = "Extend the function from the previous turn by seven and verify it."


def new_state(cancel):
    return {"secret": uuid.uuid4().hex[:12], "cancel": cancel, "results": {}, "turn": 0}


def initial_calls(state):
    secret = state["secret"]
    calls = [{"id": "create", "name": "write", "arguments": {"path": "generated.py", "content": f"def function_{secret}():\n    return 31\n"}}]
    if state["cancel"]:
        calls += [
            {"id": "held", "name": "bash", "arguments": {"command": "printf 'CONTINUE-WAIT\\n'; while true; do sleep 0.1; done"}},
            {"id": "unstarted", "name": "write", "arguments": {"path": "unstarted.txt", "content": "must not run"}},
        ]
    return calls


def next_call(state, result=None):
    secret = state["secret"]
    if result is None:
        return {"id": "read-back", "name": "read", "arguments": {"path": "generated.py"}}
    assert result["success"], result
    if result["call_id"] == "read-back":
        assert f"function_{secret}" in result["output"] and "return 31" in result["output"]
        return {"id": "extend", "name": "edit", "arguments": {"path": "generated.py", "old_text": "return 31", "new_text": "return 38"}}
    if result["call_id"] == "extend":
        return {"id": "verify", "name": "bash", "arguments": {"command": f"python3 -c 'from generated import function_{secret}; assert function_{secret}() == 38; print(\"VERIFIED-{secret}\")'"}}
    assert result["call_id"] == "verify" and result["exit_code"] == 0 and f"VERIFIED-{secret}" in result["output"]
    return None


def run(codex):
    state_path = Path("continuation-state.json")
    resumed = state_path.exists()
    state = json.loads(state_path.read_text()) if resumed else new_state(Path("continuation").read_text() == "cancel")
    identity = "fixture-thread" if codex else "fixture-session"
    attached = not resumed
    if not codex and resumed:
        attached = "--resume" in sys.argv and sys.argv[sys.argv.index("--resume") + 1] == identity

    def persist():
        state_path.write_text(json.dumps(state))

    def send(value):
        print(json.dumps(value), flush=True)

    def request(call):
        if codex:
            send({"id": call["id"], "method": "item/tool/call", "params": {"threadId": identity, "turnId": f"turn-{state['turn']}", "callId": call["id"], "tool": call["name"], "arguments": call["arguments"]}})
        else:
            send({"type": "control_request", "request_id": call["id"], "request": {"subtype": "mcp_message", "server_name": "demoncoder", "message": {"jsonrpc": "2.0", "id": call["id"], "method": "tools/call", "params": {"name": call["name"], "arguments": call["arguments"]}}}})

    def complete():
        text = "MEMORY-" + state["secret"] if state["turn"] == 1 else "CONTINUED-" + state["secret"]
        if codex:
            send({"method": "item/agentMessage/delta", "params": {"threadId": identity, "turnId": f"turn-{state['turn']}", "delta": text}})
            send({"method": "turn/completed", "params": {"threadId": identity, "turn": {"id": f"turn-{state['turn']}", "status": "completed"}}})
        else:
            send({"type": "stream_event", "session_id": identity, "event": {"delta": {"type": "text_delta", "text": text}}})
            send({"type": "result", "session_id": identity, "subtype": "success", "is_error": False})

    for raw in sys.stdin:
        message = json.loads(raw)
        is_result = (codex and "result" in message) or (not codex and message.get("type") == "control_response")
        if is_result:
            identifier = message["id"] if codex else message["response"]["request_id"]
            result = message["result"] if codex else message["response"]["response"]["mcp_response"]["result"]
            result = json.loads(result["contentItems"][0]["text"] if codex else result["content"][0]["text"])
            assert result["success"]
            assert result["call_id"] == (identifier if codex else "claude-mcp-" + identifier)
            state["results"][identifier] = result
            persist()
            if state["turn"] == 1:
                assert identifier == "create"
                if not state["cancel"]:
                    complete()
            else:
                call = next_call(state, {**result, "call_id": identifier})
                request(call) if call else complete()
            continue
        method = message.get("method") if codex else message.get("request", {}).get("subtype")
        if method == "initialize":
            if codex:
                send({"id": message["id"], "result": {"userAgent": "fixture"}})
            else:
                send({"type": "control_response", "response": {"subtype": "success", "request_id": message["request_id"], "response": {}}})
        elif method == "account/read":
            send({"id": message["id"], "result": {"account": {"type": "chatgpt"}}})
        elif method == "config/read":
            send({"id": message["id"], "result": {"config": {"mcp_servers": {}}}})
        elif method in ("thread/start", "thread/resume"):
            assert (method == "thread/resume") == resumed, "prior backend thread was discarded"
            if resumed:
                assert message["params"]["threadId"] == identity
                attached = True
            send({"id": message["id"], "result": {"thread": {"id": identity}}})
        elif method == "turn/start" or message.get("type") == "user":
            assert attached, "prior Claude session was not resumed"
            state["turn"] += 1
            prompt = message["params"]["input"][0]["text"] if codex else message["message"]["content"]
            if codex:
                assert message["params"]["threadId"] == identity
                send({"id": message["id"], "result": {"turn": {"id": f"turn-{state['turn']}"}}})
            else:
                assert state["turn"] == 1 or message["session_id"] == identity
                send({"type": "system", "subtype": "init", "session_id": identity, "apiKeySource": "none"})
            persist()
            if state["turn"] == 1:
                for call in initial_calls(state):
                    request(call)
            else:
                if Path("forget-context").exists():
                    state["results"].clear()
                assert state["turn"] == 2 and prompt == SECOND_PROMPT and state["results"]["create"]["success"]
                Path("continuation-audit.json").write_text(json.dumps({"identity": identity, "resumed": resumed, "create_result": state["results"]["create"]}))
                request(next_call(state))
