#!/usr/bin/python3
"""Controlled role and worker peers; coding effects use host-admitted tool calls."""
import json
import os
import re
import sys
import time

sys.dont_write_bytecode = True
from subagent_backend_fixture import calls_for


def work_request(prompt):
    data = prompt.split("\nAssignment: ", 1)[1]
    request, _ = json.JSONDecoder().raw_decode(data)
    match = re.search(r"Correction round: (\d+)", prompt)
    return request, int(match.group(1)) if match else 0


def fixture(request):
    return json.loads(request["context"]) if request.get("context") else {}


def work_calls(request, round_number):
    settings = fixture(request)
    if round_number:
        rounds = settings.get("corrections", [])
        return rounds[round_number - 1] if round_number <= len(rounds) else []
    return calls_for(request if "fixture_calls" in settings else {**request, "context": ""})


def role_request(prompt):
    prefix = "DemonCoder supervision role: "
    if not prompt.startswith(prefix):
        return None
    role = prompt.splitlines()[0][len(prefix):].strip()
    assert role in ("advisor", "worker_response", "judge"), role
    evidence = json.loads(prompt.split("Runtime-collected evidence:\n", 1)[1])
    return role, evidence


def role_reply(role, evidence):
    request = evidence["assignment"]
    settings = fixture(request)
    round_number = evidence["correction_round"]
    assert isinstance(evidence["source_evidence"], (str, dict, list))
    assert isinstance(evidence["checks"], list) and evidence["checks"]
    if role in ("worker_response", "judge"):
        assert isinstance(evidence["advisor"], dict), "dispute transport omitted original advisor evidence"
        assert all(field in evidence["advisor"] for field in ("verdict", "findings", "explanation"))
    if role == "judge":
        assert isinstance(evidence["response"], dict), "judge transport omitted worker response"
        assert all(field in evidence["response"] for field in ("verdict", "findings", "explanation"))
    time.sleep(settings.get("role_delay", {}).get(role, 0))
    if settings.get("role_tool") == role:
        return {"name": "write", "arguments": {"path": "forbidden-role", "content": "role escaped"}}, ""
    if settings.get("invalid_role") == role:
        return None, "invalid role verdict"
    choices = settings.get(role, ["findings" if role == "worker_response" else "clear"])
    choice = choices[min(round_number, len(choices) - 1)]
    value = choice if isinstance(choice, dict) else {
        "verdict": choice,
        "findings": ["The greeting needs the requested correction."] if choice == "findings" else [],
        "explanation": f"{role} examined round {round_number} runtime evidence and original claims.",
    }
    return None, json.dumps(value)


def send(value):
    print(json.dumps(value), flush=True)


def log_request(value):
    path = os.path.join(os.environ["HOME"], "orchestration-peer.jsonl")
    data = (json.dumps(value) + "\n").encode()
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    try:
        assert os.write(descriptor, data) == len(data), "incomplete peer request log"
    finally:
        os.close(descriptor)


def main():
    codex = "app-server" in sys.argv
    assert "OPENAI_API_KEY" not in os.environ and "ANTHROPIC_API_KEY" not in os.environ
    log_request({"kind": "launch", "adapter": "codex" if codex else "claude", "pid": os.getpid(), "start": open("/proc/self/stat").read().rsplit(")", 1)[1].split()[19]})
    pending, index, turns = [], 0, 0
    prompts_in_context = 0
    response, turn = "", ""
    exposed_tools = None
    selected_model = None if codex else sys.argv[sys.argv.index("--model") + 1]

    def next_call():
        nonlocal index
        if index < len(pending):
            call = pending[index]
            index += 1
            identifier = f"orchestration-{turn}-{index}"
            if codex:
                send({"id": identifier, "method": "item/tool/call", "params": {"threadId": "fixture-thread", "turnId": turn, "callId": identifier, "tool": call["name"], "arguments": call["arguments"]}})
            else:
                send({"type": "control_request", "request_id": identifier, "request": {"subtype": "mcp_message", "server_name": "demoncoder", "message": {"jsonrpc": "2.0", "id": identifier, "method": "tools/call", "params": call}}})
        elif codex:
            send({"method": "item/agentMessage/delta", "params": {"threadId": "fixture-thread", "turnId": turn, "delta": response}})
            send({"method": "turn/completed", "params": {"threadId": "fixture-thread", "turn": {"id": turn, "status": "completed"}}})
        else:
            send({"type": "stream_event", "session_id": "fixture-session", "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": response}}})
            send({"type": "result", "subtype": "success", "is_error": False, "session_id": "fixture-session", "usage": {"input_tokens": 12, "output_tokens": 8}})

    def begin(prompt):
        nonlocal pending, index, response, prompts_in_context
        prompts_in_context += 1
        log_request({"kind": "prompt", "adapter": "codex" if codex else "claude", "prompt": prompt, "model": selected_model})
        parsed = role_request(prompt)
        if parsed:
            assert prompts_in_context == 1, "role reused an existing conversation"
            assert "--resume" not in sys.argv and "--continue" not in sys.argv
            if codex:
                assert exposed_tools == set(), exposed_tools
            call, response = role_reply(*parsed)
            pending = [call] if call else []
            if call:
                response = json.dumps({"verdict": "clear", "findings": [], "explanation": "A denied role tool cannot authorize work."})
        else:
            if codex:
                assert exposed_tools == {"read", "write", "edit", "bash"}, exposed_tools
            request, round_number = work_request(prompt)
            pending = work_calls(request, round_number)
            response = f"ORCHESTRATION-WORK-DONE round {round_number}"
        index = 0
        next_call()

    for line in sys.stdin:
        message = json.loads(line)
        if codex:
            method, result = message.get("method"), None
            if method == "initialize":
                result = {"userAgent": "orchestration-fixture"}
            elif method == "config/read":
                result = {"config": {"mcp_servers": {}}}
            elif method == "account/read":
                result = {"requiresOpenaiAuth": True, "account": {"type": "chatgpt", "email": "fixture@example.invalid", "planType": "plus"}}
            elif method == "thread/start":
                prompts_in_context = 0
                assert message["params"]["sandbox"] == "workspace-write"
                selected_model = message["params"]["model"]
                exposed_tools = {tool["name"] for tool in message["params"]["dynamicTools"]}
                result = {"thread": {"id": "fixture-thread"}}
            elif method == "turn/start":
                turns += 1
                turn = f"turn-{turns}"
                send({"id": message["id"], "result": {"turn": {"id": turn, "status": "inProgress"}}})
                send({"method": "turn/started", "params": {"threadId": "fixture-thread", "turn": {"id": turn}}})
                begin(message["params"]["input"][0]["text"])
            elif isinstance(message.get("id"), str) and message["id"].startswith("orchestration-"):
                next_call()
            if result is not None:
                send({"id": message["id"], "result": result})
        elif message.get("type") == "control_request":
            assert message["request"]["subtype"] == "initialize"
            assert message["request"]["hooks"] is None
            send({"type": "control_response", "response": {"subtype": "success", "request_id": message["request_id"], "response": {}}})
        elif message.get("type") == "user":
            assert sys.argv[sys.argv.index("--tools") + 1] == ""
            send({"type": "system", "subtype": "init", "session_id": "fixture-session", "apiKeySource": "none"})
            begin(message["message"]["content"])
        elif message.get("type") == "control_response":
            next_call()


if __name__ == "__main__":
    main()
