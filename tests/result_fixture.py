"""A failing repository check, its correction, and the same passing check."""
import json
import shlex


def requests(adapter, token):
    check = "from answer import value; assert value == 2, " + repr("VERIFY-FAILED-" + token) + "; print(" + repr("VERIFY-PASSED-" + token) + ")"
    calls = [
        ("write", {"path": "answer.py", "content": "value = 1\n"}),
        ("bash", {"command": "python3 -B -c " + shlex.quote(check)}),
        ("edit", {"path": "answer.py", "old_text": "value = 1", "new_text": "value = 2"}),
        ("bash", {"command": "python3 -B -c " + shlex.quote(check)}),
    ]
    return [{"id": f"result-{i}", "name": "mcp__demoncoder__" + name if adapter == "claude" else name, "arguments": arguments, "success": i != 1, "adapter": adapter, "token": token} for i, (name, arguments) in enumerate(calls)]


def check_result(call, output):
    result = json.loads(output)
    assert result["success"] == call["success"], (call["id"], result)
    assert result["tool"] == call["name"].removeprefix("mcp__demoncoder__")
    if call["adapter"] != "claude":
        assert result["call_id"] == call["id"], result
    else:
        assert result["call_id"].startswith("claude-mcp-"), result
    if call["id"] == "result-1":
        assert result["exit_code"] == 1 and "AssertionError" in result["output"]
        assert "VERIFY-FAILED-" + call["token"] in result["output"]
    elif call["id"] == "result-3":
        assert result["exit_code"] == 0
        assert result["output"].strip() == "VERIFY-PASSED-" + call["token"]
    else:
        assert result["exit_code"] is None


def validate(workspace, server, records, terminal_output):
    assert (workspace / "answer.py").read_text() == "value = 2\n"
    received = [json.loads(result) for result in server.results]
    actual = [row["result"] for row in records if row["type"] == "tool_finished"]
    assert actual == received and len(actual) == 4, "retained tool evidence differs from the model results"
    assert len({result["call_id"] for result in actual}) == 4, "tool call identity was reused"
    assert server.failure_rendered.is_set(), "failed check was not visible before correction"
    assert ("VERIFY-FAILED-" + server.token).encode() in terminal_output
    assert not actual[1]["success"] and actual[3]["success"]
