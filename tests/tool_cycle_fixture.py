"""A controlled model decision sequence; this module never changes workspace files."""
import json


class Cycle:
    def __init__(self, token, wrong_edit=False):
        self.token = token
        self.wrong_edit = wrong_edit
        self.step = 0
        self.seed = None
        self.last_call = None

    def next(self, result=None):
        if result is not None:
            if self.step == 4 and self.wrong_edit:
                assert not result["success"], "fault must fail the real verification command"
                return None
            assert result["success"], result["output"]
            assert result["tool"] == self.last_call["name"]
            if self.step == 1:
                self.seed = int(result["output"].strip())
            if self.step == 4:
                assert result["exit_code"] == 0
                assert "VERIFIED-" + self.token in result["output"]
                return None
        if self.step == 0:
            name, arguments = "read", {"path": "seed.txt"}
        elif self.step == 1:
            name, arguments = "write", {"path": "answer.py", "content": f"value = {self.seed}\n"}
        elif self.step == 2:
            name, arguments = "edit", {"path": "answer.py", "old_text": f"value = {self.seed}", "new_text": f"value = {self.seed + (2 if self.wrong_edit else 1)}"}
        elif self.step == 3:
            name, arguments = "bash", {"command": f"python3 -B -c 'from answer import value; assert value == {self.seed + 1}; print(\"VERIFIED-{self.token}\")'"}
        else:
            raise AssertionError("unexpected cycle state")
        self.step += 1
        self.last_call = {"id": f"{self.token}-call-{self.step}", "name": name, "arguments": arguments}
        return self.last_call


def sse_call(path, call):
    if path == "/responses":
        return [{"type": "response.completed", "response": {"output": [{
            "type": "function_call", "call_id": call["id"], "name": call["name"], "arguments": json.dumps(call["arguments"]),
        }]}}]
    arguments = json.dumps(call["arguments"])
    midpoint = len(arguments) // 2
    return [
        {"type": "message_start", "message": {"usage": {"input_tokens": 9}}},
        {"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": call["id"], "name": call["name"], "input": {}}},
        *[{"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": part}} for part in (arguments[:midpoint], arguments[midpoint:])],
        {"type": "content_block_stop", "index": 0},
        {"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 20}},
        {"type": "message_stop"},
    ]
