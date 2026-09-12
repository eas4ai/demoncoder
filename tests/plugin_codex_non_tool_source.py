#!/usr/bin/env python3
"""Observe pinned Codex submit and Stop command hooks with a synthetic model."""

import argparse
import copy
import hashlib
import json
import re
from pathlib import Path

import codex_https_fixture
import installed_backends
import plugin_codex_post_source as peer

PIN = "80315a32acf1b625129a46b0bd75537cf76ef09701ae986154159076cc0b6aff"
CASES = {
    "pass": (1, [False]),
    "submit-deny": (0, []),
    "stop-correct": (2, [False, True]),
}
PROMPT = "Perform the fixture operation once."


def verify(case, result, hooks, times, events, requests, trace, root):
    count, active = CASES[case]
    assert result["case"] == case and result["errors"] == []
    assert len(requests) == result["model_requests"] == count
    assert [r["message"] for r in trace if r["direction"] == "backend"] == events
    models = [r for r in trace if r["direction"] == "model"]
    assert [r["message"] for r in models] == requests
    thread_reply = next(m["result"]["thread"] for m in events if m.get("id") == 3)
    turn_reply = next(m["result"]["turn"] for m in events if m.get("id") == 4)
    assert result["thread"] == thread_reply["id"]
    assert result["transcript"] == thread_reply["path"]
    assert result["turn"] == turn_reply["id"]
    assert [h["hook_event_name"] for h in hooks] == ["UserPromptSubmit"] + [
        "Stop"
    ] * len(active)
    assert [r["event"] for r in times] == hooks
    assert len(times) == len(hooks)
    for index, hook in enumerate(hooks):
        expected = {
            "session_id",
            "turn_id",
            "transcript_path",
            "cwd",
            "hook_event_name",
            "model",
            "permission_mode",
        }
        expected |= (
            {"prompt"} if index == 0 else {"stop_hook_active", "last_assistant_message"}
        )
        assert set(hook) == expected
        assert hook["session_id"] == result["thread"]
        assert hook["turn_id"] == result["turn"]
        assert hook["transcript_path"] == result["transcript"]
        assert isinstance(hook["transcript_path"], str)
        assert Path(hook["transcript_path"]).is_relative_to(root / "codex-home")
        assert hook["cwd"] == str(root / "work")
        assert (
            hook["model"] == "gpt-5.4"
            and hook["permission_mode"] == "bypassPermissions"
        )
        assert type(times[index]["time"]) in (float, int)
        if index:
            assert hook["stop_hook_active"] is active[index - 1]
            assert hook["last_assistant_message"] == "done"
            assert models[index - 1]["time"] < times[index]["time"]
        else:
            assert hook["prompt"] == PROMPT
    turns = [
        r["message"]
        for r in trace
        if r["direction"] == "host" and r["message"].get("method") == "turn/start"
    ]
    assert len(turns) == 1
    assert turns[0]["params"]["threadId"] == result["thread"]
    assert turns[0]["params"]["input"] == [{"type": "text", "text": PROMPT}]
    started = [m for m in events if m.get("method") == "hook/started"]
    completed = [m for m in events if m.get("method") == "hook/completed"]
    expected_names = ["userPromptSubmit"] + ["stop"] * len(active)
    assert [m["params"]["run"]["eventName"] for m in started] == expected_names
    assert [m["params"]["run"]["eventName"] for m in completed] == expected_names
    for index, (start, end) in enumerate(zip(started, completed, strict=True)):
        assert events.index(start) < events.index(end)
        for message in [start, end]:
            params = message["params"]
            assert (
                params["threadId"] == result["thread"]
                and params["turnId"] == result["turn"]
            )
            assert params["run"]["handlerType"] == "command"
            assert params["run"]["executionMode"] == "sync"
            assert params["run"]["sourcePath"] == str(root / "codex-home/config.toml")
        assert start["params"]["run"]["id"] == end["params"]["run"]["id"]
        blocked = (case == "submit-deny" and index == 0) or (
            case == "stop-correct" and index == 1
        )
        assert end["params"]["run"]["status"] == ("blocked" if blocked else "completed")
        if blocked:
            reason = (
                "CODEX_SOURCE_SUBMIT_DENY"
                if index == 0
                else "CODEX_SOURCE_STOP_CORRECTION"
            )
            assert any(
                entry["text"] == reason for entry in end["params"]["run"]["entries"]
            )
    assistant_positions = [
        i
        for i, m in enumerate(events)
        if m.get("method") == "item/completed"
        and m["params"]["item"]["type"] == "agentMessage"
    ]
    assert len(assistant_positions) == count
    for index, position in enumerate(assistant_positions):
        message = events[position]["params"]
        assert (
            message["threadId"] == result["thread"]
            and message["turnId"] == result["turn"]
        )
        assert message["item"]["text"] == "done"
        assert position < events.index(started[index + 1])
    terminals = [m for m in events if m.get("method") == "turn/completed"]
    assert len(terminals) == 1
    terminal = terminals[0]["params"]
    assert (
        terminal["threadId"] == result["thread"]
        and terminal["turn"]["id"] == result["turn"]
    )
    assert (
        terminal["turn"]["status"] == "completed" and terminal["turn"]["error"] is None
    )
    assert events.index(completed[-1]) < events.index(terminals[0])
    assert not any(m.get("method") == "item/tool/call" for m in events)
    if models:
        assert times[0]["time"] < models[0]["time"]
        assert (
            next(
                r["time"]
                for r in trace
                if r["direction"] == "backend" and r["message"] == completed[0]
            )
            < models[0]["time"]
        )
    if case == "stop-correct":
        assert times[1]["time"] < models[1]["time"]
        assert (
            next(
                r["time"]
                for r in trace
                if r["direction"] == "backend" and r["message"] == completed[1]
            )
            < models[1]["time"]
        )
        correction_run = completed[1]["params"]["run"]["id"]
        assert any(
            item.get("role") == "user"
            and isinstance(item.get("content"), list)
            and any(
                block.get("type") == "input_text"
                and block.get("text")
                == f'<hook_prompt hook_run_id="{correction_run}">CODEX_SOURCE_STOP_CORRECTION</hook_prompt>'
                for block in item["content"]
            )
            for item in requests[1]["input"]
        )


def attack(case, result, hooks, times, events, requests, trace, root):
    original = [result, hooks, times, events, requests, trace]
    variants = []
    for field, value in [
        ("session_id", "wrong"),
        ("turn_id", "wrong"),
        ("cwd", "/wrong"),
        ("prompt", "wrong"),
        ("hook_event_name", "PreToolUse"),
    ]:
        bad = copy.deepcopy(original)
        bad[1][0][field] = value
        bad[2][0]["event"][field] = value
        variants.append(bad)
    bad = copy.deepcopy(original)
    bad[0]["model_requests"] = 99
    variants.append(bad)
    bad = copy.deepcopy(original)
    bad[0]["transcript"] = str(root / "codex-home/fabricated.jsonl")
    for hook, timing in zip(bad[1], bad[2], strict=True):
        hook["transcript_path"] = bad[0]["transcript"]
        timing["event"]["transcript_path"] = bad[0]["transcript"]
    variants.append(bad)
    if requests:
        bad = copy.deepcopy(original)
        bad[1][1]["stop_hook_active"] = True
        bad[2][1]["event"]["stop_hook_active"] = True
        variants.append(bad)
        bad = copy.deepcopy(original)
        bad[2][0]["time"] = (
            next(r["time"] for r in trace if r["direction"] == "model") + 1
        )
        variants.append(bad)
        bad = copy.deepcopy(original)
        bad[3] = [
            m
            for m in bad[3]
            if not (
                m.get("method") == "item/completed"
                and m["params"]["item"]["type"] == "agentMessage"
            )
        ]
        bad[5] = [
            r for r in bad[5] if r["direction"] != "backend" or r["message"] in bad[3]
        ]
        variants.append(bad)
    if case == "stop-correct":
        bad = copy.deepcopy(original)
        for item in bad[4][1]["input"]:
            for block in item.get("content", []):
                if isinstance(block, dict) and "text" in block:
                    block["text"] = re.sub(
                        r'hook_run_id="[^"]+"', 'hook_run_id="unrelated"', block["text"]
                    )
        [r for r in bad[5] if r["direction"] == "model"][1]["message"] = copy.deepcopy(
            bad[4][1]
        )
        variants.append(bad)
        bad = copy.deepcopy(original)
        completed_stop = next(
            r
            for r in bad[5]
            if r["direction"] == "backend"
            and r["message"].get("method") == "hook/completed"
            and r["message"]["params"]["run"]["eventName"] == "stop"
        )
        bad[5].remove(completed_stop)
        second_model = [r for r in bad[5] if r["direction"] == "model"][1]
        completed_stop["time"] = second_model["time"] + 0.000001
        bad[5].insert(bad[5].index(second_model) + 1, completed_stop)
        bad[3] = [r["message"] for r in bad[5] if r["direction"] == "backend"]
        variants.append(bad)
        bad = copy.deepcopy(original)
        bad[4][1]["input"] = [
            {"role": "user", "content": [], "unused": "CODEX_SOURCE_STOP_CORRECTION"}
        ]
        [r for r in bad[5] if r["direction"] == "model"][1]["message"] = copy.deepcopy(
            bad[4][1]
        )
        variants.append(bad)
    for bad in variants:
        try:
            verify(case, *bad, root)
        except (AssertionError, KeyError, StopIteration):
            continue
        raise AssertionError("verifier accepted corrupted source evidence")
    return len(variants)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not __debug__:
        raise RuntimeError("optimized Python disables qualification assertions")
    binary = args.codex.resolve(strict=True)
    assert (
        hashlib.sha256(binary.read_bytes()).hexdigest() == PIN
    ), "wrong executable pin"
    root = args.output.resolve()
    root.mkdir(mode=0o700)
    inputs = [
        Path(__file__).resolve(),
        Path(peer.__file__).resolve(),
        peer.FIXTURE.resolve(),
        Path(codex_https_fixture.__file__).resolve(),
        Path(installed_backends.__file__).resolve(),
    ]
    frozen = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs}
    for path in inputs:
        (root / path.name).write_bytes(path.read_bytes())
    fixture = json.loads(peer.FIXTURE.read_text())
    reports = []
    for case in CASES:
        destination = root / case
        outcome = peer.run_case(binary, destination, fixture, True, lifecycle=case)
        (destination / "result.json").write_text(json.dumps(outcome, indent=2) + "\n")
        hooks, times = [
            [json.loads(line) for line in (destination / name).read_text().splitlines()]
            for name in ["hooks.jsonl", "hooks.jsonl.times"]
        ]
        events, requests, trace = [
            json.loads((destination / name).read_text())
            for name in ["events.json", "model-requests.json", "trace.json"]
        ]
        verify(case, outcome, hooks, times, events, requests, trace, destination)
        reports.append(
            {
                "case": case,
                "corruptions_rejected": attack(
                    case, outcome, hooks, times, events, requests, trace, destination
                ),
            }
        )
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == PIN
    assert frozen == {
        str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs
    }
    paths = [root / p.name for p in inputs]
    for case in CASES:
        paths += [
            root / case / name
            for name in [
                "result.json",
                "hooks.jsonl",
                "hooks.jsonl.times",
                "events.json",
                "model-requests.json",
                "trace.json",
                "inputs.json",
                "capture.py",
                "codex-home/config.toml",
                "stderr",
            ]
        ]
    result = {
        "kind": "controlled-pinned-source",
        "executable_sha256": PIN,
        "cases": reports,
        "inputs": frozen,
        "artifacts": [
            {
                "path": str(p.relative_to(root)),
                "sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
            }
            for p in paths
        ],
        "limits": "Actual pinned source command hooks with synthetic local model. Not DemonCoder lifecycle, relay authentication, host correction limits, cancellation, MCP handlers or live provider qualification. Stdout and model input bounded; stderr captured without byte cap.",
    }
    (root / "qualification.json").write_text(json.dumps(result, indent=2) + "\n")
    print(
        f"PASS: {len(reports)} source cases, {sum(r['corruptions_rejected'] for r in reports)} corrupted-evidence rejections"
    )


if __name__ == "__main__":
    main()
