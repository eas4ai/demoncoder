"""Harmless requests that challenge the final tool boundary."""
import json
import shlex


def requests(adapter, parent, secret):
    outside = str(parent / "outside" / "canary.txt")
    marker = str(parent / "outside" / "bypass.txt")
    home_canary = str(parent / "home" / ".demoncoder" / "credential-canary")
    commands = [
        ("write", {"path": "allowed.txt", "content": "first"}, True),
        ("read", {"path": "allowed.txt"}, True),
        ("edit", {"path": "allowed.txt", "old_text": "first", "new_text": "second"}, True),
        ("read", {"path": outside}, True),
        ("read", {"path": "../outside/canary.txt"}, True),
        ("write", {"path": "../outside/bypass.txt", "content": "harmless marker"}, False),
        ("edit", {"path": outside, "old_text": "OUTSIDE-DOCUMENTATION", "new_text": "changed"}, False),
        ("write", {"path": ".git/protected", "content": "changed"}, False),
        ("bash", {"command": "python3 -B -c " + shlex.quote("import os,pathlib; assert not any(k in os.environ for k in ['OPENAI_API_KEY','ANTHROPIC_API_KEY','CLAUDE_CODE_OAUTH_TOKEN','CODEX_HOME']); assert pathlib.Path(" + repr(outside) + ").read_text() == 'OUTSIDE-DOCUMENTATION'; assert not pathlib.Path(" + repr(home_canary) + ").exists(); print('ISOLATED')")}, True),
        ("bash", {"command": "printf harmless > " + shlex.quote(marker)}, False),
        ("bash", {"command": "printf changed > .demoncoder/protected"}, False),
    ]
    commands += [("read", {"path": str(parent / "home/.codex/BEST_PRACTICES.md")}, True)]
    calls = [{"id": f"boundary-{i}", "name": ("mcp__demoncoder__" + tool if adapter == "claude" else tool), "arguments": args, "success": success, "host": True} for i, (tool, args, success) in enumerate(commands)]
    if adapter == "codex":
        calls += [
            {"id": "builtin-patch", "name": "apply_patch", "input": f"*** Begin Patch\n*** Add File: {marker}\n+harmless marker\n*** End Patch", "host": False},
            {"id": "builtin-shell", "name": "exec_command", "arguments": {"cmd": "printf harmless > " + shlex.quote(marker)}, "host": False},
        ]
    elif adapter == "claude":
        calls += [
            {"id": "builtin-read", "name": "Read", "arguments": {"file_path": outside}, "host": False},
            {"id": "builtin-shell", "name": "Bash", "arguments": {"command": "printf harmless > " + shlex.quote(marker)}, "host": False},
        ]
    return calls


def model_results(body, openai):
    if openai:
        return {item["call_id"]: text_content(item["output"]) for item in body["input"] if item.get("type") in ["function_call_output", "custom_tool_call_output"]}
    results = {}
    for message in body["messages"]:
        if not isinstance(message["content"], list):
            continue
        for block in message["content"]:
            if block["type"] == "tool_result":
                content = block.get("content", "")
                if isinstance(content, list):
                    content = "\n".join(item.get("text", "") for item in content)
                results[block["tool_use_id"]] = content
    return results


def text_content(content):
    if isinstance(content, list):
        return "\n".join(item.get("text", "") for item in content)
    return content


def check_result(call, output):
    if call["host"]:
        result = json.loads(output)
        assert result["success"] == call["success"], (call["id"], result)
        if call["id"] == "boundary-1":
            assert result["output"] == "first"
        if call["id"] in ("boundary-3", "boundary-4"):
            assert result["output"] == "OUTSIDE-DOCUMENTATION"
        if call["id"] == "boundary-11":
            assert result["output"] == "MACHINE-STANDARDS"
        if call["id"] == "boundary-8":
            assert result["exit_code"] == 0 and "ISOLATED" in result["output"]
    else:
        assert any(word in str(output).lower() for word in ["unknown", "not available", "not found", "unsupported", "unavailable", "no such tool"]), output
