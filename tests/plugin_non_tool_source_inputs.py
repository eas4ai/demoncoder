#!/usr/bin/env python3
"""Qualify pinned Claude no-tool submit and Stop callbacks with a local peer.

This checks source behavior, not host lifecycle delivery or live provider access.
"""

import argparse
import copy
import hashlib
import json
from pathlib import Path

import plugin_post_source_inputs as peer

PIN = "0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0"
PROMPT = "Perform the fixture operation once."
CASES = {
    "pass": (1, [False]),
    "submit-deny": (0, []),
    "stop-correct": (2, [False, True]),
}


def verify(case, events, requests, trace, root):
    expected_requests, active = CASES[case]
    assert len(requests) == expected_requests
    assert [row["message"] for row in trace if row["direction"] == "model"] == requests
    assert [row["message"] for row in trace if row["direction"] == "backend"] == events
    hooks = [
        m for m in events if m.get("request", {}).get("subtype") == "hook_callback"
    ]
    assert [m["request"]["callback_id"] for m in hooks] == ["UserPromptSubmit"] + [
        "Stop"
    ] * len(active)
    results = [m for m in events if m.get("type") == "result"]
    assert len(results) == 1
    result = results[0]
    assert result["is_error"] is False and result["subtype"] == "success"
    assert result["num_turns"] == expected_requests
    prompts = [
        row["message"]
        for row in trace
        if row["direction"] == "host" and row["message"].get("type") == "user"
    ]
    assert len(prompts) == 1 and prompts[0]["message"] == {
        "role": "user",
        "content": PROMPT,
    }
    assert result["user_message_uuid"] == prompts[0]["uuid"]
    assert not any(
        m.get("request", {}).get("message", {}).get("method") == "tools/call"
        for m in events
    )
    session = result["session_id"]
    assert isinstance(session, str) and session
    base = hooks[0]["request"]["input"]
    assert base["prompt"] == PROMPT
    assert isinstance(base["transcript_path"], str) and base["transcript_path"]
    assert Path(base["transcript_path"]).is_relative_to(root / "home")
    assert isinstance(base["prompt_id"], str) and base["prompt_id"]
    callback_ids = []
    for index, hook in enumerate(hooks):
        request = hook["request"]
        frame = request["input"]
        assert frame["session_id"] == session
        assert frame["cwd"] == str(root / "work")
        assert frame["transcript_path"] == base["transcript_path"]
        assert frame["prompt_id"] == base["prompt_id"]
        assert frame["permission_mode"] == "default"
        assert frame["hook_event_name"] == request["callback_id"]
        assert not any(
            key in frame for key in ["tool_name", "tool_input", "tool_response"]
        )
        # SDK callback correlation is not evidence that a tool operation occurred.
        assert isinstance(request["tool_use_id"], str) and request["tool_use_id"]
        callback_ids.append(hook["request_id"])
        if index:
            assert frame["stop_hook_active"] is active[index - 1]
            assert frame["last_assistant_message"] == "done"
    assert len(callback_ids) == len(set(callback_ids))
    positions = []
    for hook in hooks:
        received = next(
            i
            for i, row in enumerate(trace)
            if row["direction"] == "backend" and row["message"] == hook
        )
        replies = [
            (i, row["message"]["response"])
            for i, row in enumerate(trace)
            if row["direction"] == "host"
            and row["message"].get("type") == "control_response"
            and row["message"]["response"].get("request_id") == hook["request_id"]
        ]
        assert len(replies) == 1
        sent, reply = replies[0]
        assert received < sent and reply["subtype"] == "success"
        name = hook["request"]["callback_id"]
        blocked = (case == "submit-deny" and name == "UserPromptSubmit") or (
            case == "stop-correct"
            and name == "Stop"
            and not hook["request"]["input"]["stop_hook_active"]
        )
        expected = (
            {
                "decision": "block",
                "reason": (
                    "SOURCE_SUBMIT_DENY"
                    if name == "UserPromptSubmit"
                    else "SOURCE_STOP_CORRECTION"
                ),
            }
            if blocked
            else {}
        )
        assert reply["response"] == expected
        positions.append((received, sent))
    models = [i for i, row in enumerate(trace) if row["direction"] == "model"]
    terminal = next(
        i
        for i, row in enumerate(trace)
        if row["direction"] == "backend" and row["message"] == result
    )
    assert positions[-1][1] < terminal
    assistants = [
        (i, row["message"])
        for i, row in enumerate(trace)
        if row["direction"] == "backend" and row["message"].get("type") == "assistant"
    ]
    assert len(assistants) == expected_requests
    for index, (received, assistant) in enumerate(assistants):
        assert models[index] < received < positions[index + 1][0]
        assert assistant["session_id"] == session
        assert assistant["message"]["role"] == "assistant"
        assert assistant["message"]["content"] == [{"type": "text", "text": "done"}]
    if models:
        assert positions[0][1] < models[0] < positions[1][0]
        assert result["result"] == "done"
    if case == "stop-correct":
        assert positions[1][1] < models[1] < positions[2][0]
        assert any(
            message.get("role") == "user"
            and isinstance(message.get("content"), list)
            and any(
                block.get("type") == "text"
                and block.get("text") == "Stop hook feedback:\nSOURCE_STOP_CORRECTION"
                for block in message["content"]
            )
            for message in requests[1]["messages"]
        )
    if case == "submit-deny":
        assert "SOURCE_SUBMIT_DENY" in result["result"] and PROMPT in result["result"]
        assert result["total_cost_usd"] == 0


def attacks(case, events, requests, trace, root):
    count = 0

    def reject(e, r, t):
        nonlocal count
        try:
            verify(case, e, r, t, root)
        except (AssertionError, KeyError, StopIteration):
            count += 1
            return
        raise AssertionError("source verifier accepted corrupted evidence")

    # Mutate both copies so cross-artifact equality alone cannot reject the attack.
    def mutate_event(predicate, change):
        e, t = copy.deepcopy(events), copy.deepcopy(trace)
        change(next(m for m in e if predicate(m)))
        change(
            next(
                row["message"]
                for row in t
                if row["direction"] == "backend" and predicate(row["message"])
            )
        )
        reject(e, requests, t)

    def is_submit(m):
        return m.get("request", {}).get("callback_id") == "UserPromptSubmit"

    for field, value in [
        ("session_id", "wrong"),
        ("cwd", "/wrong"),
        ("prompt", "wrong"),
        ("hook_event_name", "PreToolUse"),
        ("transcript_path", "/wrong"),
        ("tool_name", "fabricated"),
    ]:
        mutate_event(
            is_submit,
            lambda m, f=field, v=value: m["request"]["input"].__setitem__(f, v),
        )
    mutate_event(
        lambda m: m.get("type") == "result", lambda m: m.__setitem__("num_turns", 99)
    )
    if case != "submit-deny":
        mutate_event(
            lambda m: m.get("request", {}).get("callback_id") == "Stop",
            lambda m: m["request"]["input"].__setitem__("stop_hook_active", True),
        )
    t = copy.deepcopy(trace)
    reply = next(
        row["message"]["response"]
        for row in t
        if row["direction"] == "host"
        and row["message"].get("type") == "control_response"
        and row["message"]["response"].get("request_id")
        == next(m for m in events if is_submit(m))["request_id"]
    )
    reply["response"] = {"decision": "allow"}
    reject(events, requests, t)
    if requests:
        t = copy.deepcopy(trace)
        model = next(row for row in t if row["direction"] == "model")
        t.remove(model)
        t.insert(0, model)
        reject(events, requests, t)
        e = [
            m
            for m in copy.deepcopy(events)
            if m.get("type") not in ["assistant", "stream_event"]
        ]
        t = [
            row
            for row in copy.deepcopy(trace)
            if not (
                row["direction"] == "backend"
                and row["message"].get("type") in ["assistant", "stream_event"]
            )
        ]
        reject(e, requests, t)
    if case == "stop-correct":
        r, t = copy.deepcopy(requests), copy.deepcopy(trace)
        r[1]["messages"] = [
            {
                "role": "user",
                "content": "Nothing corrective here",
                "unused_field": "SOURCE_STOP_CORRECTION",
            }
        ]
        [row for row in t if row["direction"] == "model"][1]["message"] = copy.deepcopy(
            r[1]
        )
        reject(events, r, t)
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claude", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not __debug__:
        raise RuntimeError("optimized Python disables qualification assertions")
    binary = args.claude.resolve(strict=True)
    assert (
        hashlib.sha256(binary.read_bytes()).hexdigest() == PIN
    ), "wrong executable pin"
    root = args.output.resolve()
    root.mkdir(mode=0o700)
    inputs = [
        Path(__file__).resolve(),
        Path(peer.__file__).resolve(),
        peer.FIXTURE.resolve(),
    ]
    frozen = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs}
    for path in inputs:
        (root / path.name).write_bytes(path.read_bytes())
    fixture = json.loads(peer.FIXTURE.read_text())
    reports = []
    for case in CASES:
        destination = root / case
        outcome = peer.run_case(binary, destination, fixture, "success", lifecycle=case)
        events, requests, trace = [
            json.loads((destination / name).read_text())
            for name in ["events.json", "model-requests.json", "trace.json"]
        ]
        verify(case, events, requests, trace, destination)
        outcome["corruptions_rejected"] = attacks(
            case, events, requests, trace, destination
        )
        reports.append(outcome)
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == PIN
    assert frozen == {
        str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs
    }
    result = {
        "kind": "controlled-pinned-source",
        "executable_sha256": PIN,
        "cases": reports,
        "limits": "Actual no-tool SDK callback framing and source correction/denial with local synthetic model. Not DemonCoder delivery, host correction accounting, cancellation, command handlers, or live-provider qualification.",
        "inputs": [
            {"path": str(p), "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
            for p in inputs
        ],
        "artifacts": [
            {
                "path": str(p.relative_to(root)),
                "sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
            }
            for p in sorted(
                [
                    *root.glob("*/*.json"),
                    *root.glob("*/stderr"),
                    *[root / p.name for p in inputs],
                ]
            )
        ],
    }
    (root / "qualification.json").write_text(json.dumps(result, indent=2) + "\n")
    print(
        f"PASS: {len(reports)} source cases, {sum(r['corruptions_rejected'] for r in reports)} corrupted-evidence rejections"
    )


if __name__ == "__main__":
    main()
