#!/usr/bin/env python3
"""Qualify pinned Claude command async transfer and idle rewake with local peers."""

import argparse
import copy
import hashlib
import json
from pathlib import Path

import plugin_once_source_inputs as source

FIXTURE = Path(__file__).parent / "fixtures/plugins/claude-async-source.json"
FILES = [
    "probe.py",
    "capture.py",
    "inputs.json",
    "events.json",
    "model-requests.json",
    "trace.json",
    "result.json",
    "stderr",
    "work/.claude/skills/onceprobe/SKILL.md",
    "calls.jsonl",
]


def load(root):
    return {
        "case": json.loads((root / "result.json").read_text()),
        "inputs": json.loads((root / "inputs.json").read_text()),
        "events": json.loads((root / "events.json").read_text()),
        "requests": json.loads((root / "model-requests.json").read_text()),
        "trace": json.loads((root / "trace.json").read_text()),
        "raw": [
            json.loads(line) for line in (root / "calls.jsonl").read_text().splitlines()
        ],
        "skill": (root / "work/.claude/skills/onceprobe/SKILL.md").read_text(),
        "capture": (root / "capture.py").read_text(),
    }


def verify(data, fixture):
    case = data["case"]
    expected = fixture["cases"][case["case"]]
    observer = expected["observer"]
    inputs = data["inputs"]
    assert case["failure"] is None and case["server_errors"] == []
    assert inputs["executable_sha256"] == fixture["executable_sha256"]
    assert inputs["observer_probe"] == observer
    assert inputs["once"] is True and inputs["exit"] == expected["exit_code"]
    assert inputs["async"] == expected["background"]
    assert inputs["skill"] == data["skill"]
    configured_async = expected["background"] and not observer["first_line"]
    assert f"\n          async: {str(configured_async).lower()}\n" in data["skill"]
    assert (
        f"\n          asyncRewake: {str(observer['rewake']).lower()}\n" in data["skill"]
    )
    assert "time.sleep(0.4)" in data["capture"]
    assert f"sys.exit({expected['exit_code']})" in data["capture"]
    assert ('"asyncTimeout":100' in data["capture"]) == observer["first_line"]
    assert case["hook_log"] == data["raw"]
    trace = data["trace"]
    events = data["events"]
    requests = data["requests"]
    assert events == [row["message"] for row in trace if row["direction"] == "backend"]
    assert len(requests) == case["model_requests"] == expected["model_requests"]
    model = [row for row in trace if row["direction"] == "model"]
    assert [row["index"] for row in model] == list(range(1, len(requests) + 1))
    marker = (
        "ASYNC_REWAKE_PROBE_MARKER"
        if observer["idle"]
        else "DYNAMIC_ASYNC_CONTEXT_MARKER"
    )
    assert [
        any(
            message["role"] == "user"
            and isinstance(message["content"], list)
            and any(
                block.get("type") == "text" and marker in block.get("text", "")
                for block in message["content"]
            )
            for message in request["messages"]
        )
        for request in requests
    ] == expected["context_per_request"]
    results = [
        row
        for row in trace
        if row["direction"] == "backend" and row["message"].get("type") == "result"
    ]
    assert len(results) == case["result_count"] == expected["result_count"]
    assert all(row["message"].get("is_error") is False for row in results)
    assert len({row["message"]["session_id"] for row in results}) == 1
    session = results[0]["message"]["session_id"]
    starts = [row for row in data["raw"] if row["stage"] == "start"]
    ends = [row for row in data["raw"] if row["stage"] == "end"]
    assert len(starts) + len(ends) == len(data["raw"])
    assert len(starts) == len(ends) == len(expected["hook_steps"])
    assert [row["event"]["tool_input"]["step"] for row in starts] == expected[
        "hook_steps"
    ]
    for start in starts:
        event = start["event"]
        assert (
            event["hook_event_name"] == "PostToolUse"
            and event["tool_name"] == source.TOOL
        )
        assert event["tool_use_id"] == f"call_{event['tool_input']['step']}"
        assert event["session_id"] == session
        assert event["tool_response"] == [{"type": "text", "text": "captured"}]
        end = next(row for row in ends if row["pid"] == start["pid"])
        assert end["event"] == event and end["time"] - start["time"] >= 0.3
    if not observer["idle"]:
        source.verify(case, events, requests, trace, fixture)
        first_end = next(
            row["time"] for row in ends if row["event"]["tool_input"]["step"] == 1
        )
        if observer["first_line"]:
            assert model[1]["time"] < first_end
        else:
            assert model[1]["time"] > first_end
        return
    users = [
        row
        for row in trace
        if row["direction"] == "host" and row["message"].get("type") == "user"
    ]
    assert len(users) == 1 and users[0]["message"]["message"]["content"] == "/onceprobe"
    assert case["skill_loaded_per_request"] == [True] * len(requests)
    assert all(source.MARKER in json.dumps(request) for request in requests)
    calls = [
        row
        for row in trace
        if row["direction"] == "backend"
        and row["message"].get("request", {}).get("message", {}).get("method")
        == "tools/call"
    ]
    assert len(calls) == case["tool_calls"] == 1
    request = calls[0]["message"]["request"]
    assert request["server_name"] == "demoncoder"
    params = request["message"]["params"]
    assert params["name"] == "capture" and params["arguments"] == {"step": 1}
    assert params["_meta"]["claudecode/toolUseId"] == "call_1"
    assert results[0]["time"] < ends[0]["time"]
    assert case["observed_until"] >= results[0]["time"] + 1.9
    assert case["observed_until"] >= ends[0]["time"] + 0.5
    if expected["result_count"] == 2:
        assert model[2]["time"] > ends[0]["time"]


def attack(data, fixture):
    mutations = []

    def changed(edit):
        bad = copy.deepcopy(data)
        edit(bad)
        mutations.append(bad)

    changed(lambda d: d["case"].update(failure="interrupted"))
    changed(lambda d: d["case"].update(model_requests=99))
    changed(lambda d: d["case"].update(tool_calls=99))
    changed(lambda d: d["case"].update(result_count=99))
    changed(lambda d: d.update(raw=[]))
    changed(lambda d: d["inputs"].update(executable_sha256="wrong"))
    changed(lambda d: d["inputs"].update(observer_probe={}))
    changed(lambda d: d.update(skill="wrong"))
    changed(lambda d: d.update(capture="wrong"))
    changed(lambda d: d.update(events=[]))
    changed(lambda d: d.update(requests=[]))
    changed(lambda d: d.update(trace=[]))
    changed(lambda d: d["case"].update(skill_loaded_per_request=[]))
    changed(
        lambda d: d["requests"][0]["messages"].append(
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": "ASYNC_REWAKE_PROBE_MARKER DYNAMIC_ASYNC_CONTEXT_MARKER",
                    }
                ],
            }
        )
    )

    def alter_backend(d, predicate, edit):
        event = next(row for row in d["events"] if predicate(row))
        edit(event)
        row = next(
            row
            for row in d["trace"]
            if row["direction"] == "backend" and predicate(row["message"])
        )
        edit(row["message"])

    changed(
        lambda d: alter_backend(
            d,
            lambda row: row.get("type") == "result",
            lambda row: row.update(is_error=True),
        )
    )
    changed(
        lambda d: alter_backend(
            d,
            lambda row: row.get("request", {}).get("message", {}).get("method")
            == "tools/call",
            lambda row: row["request"].update(server_name="wrong-server"),
        )
    )

    def remove_elapsed(d):
        for rows in [d["raw"], d["case"]["hook_log"]]:
            start = next(row for row in rows if row["stage"] == "start")
            end = next(row for row in rows if row["stage"] == "end")
            end["time"] = start["time"]

    changed(remove_elapsed)
    observer = fixture["cases"][data["case"]["case"]]["observer"]
    if not observer["idle"]:

        def reverse_order(d):
            end = next(row["time"] for row in d["raw"] if row["stage"] == "end")
            model2 = next(
                row
                for row in d["trace"]
                if row["direction"] == "model" and row["index"] == 2
            )
            model2["time"] = end + (0.01 if observer["first_line"] else -0.01)

        changed(reverse_order)
    if any(fixture["cases"][data["case"]["case"]]["context_per_request"]):

        def move_context_to_metadata(d):
            for request in d["requests"]:
                request["messages"] = json.loads(
                    json.dumps(request["messages"])
                    .replace("ASYNC_REWAKE_PROBE_MARKER", "removed")
                    .replace("DYNAMIC_ASYNC_CONTEXT_MARKER", "removed")
                )
                request["metadata"] = {
                    "unrelated": "ASYNC_REWAKE_PROBE_MARKER DYNAMIC_ASYNC_CONTEXT_MARKER"
                }

        changed(move_context_to_metadata)
    if fixture["cases"][data["case"]["case"]]["observer"]["idle"]:
        changed(lambda d: d["case"].update(observed_until=0))
        changed(
            lambda d: d["trace"].append(
                next(
                    row
                    for row in d["trace"]
                    if row["direction"] == "host"
                    and row["message"].get("type") == "user"
                )
            )
        )
    for bad in mutations:
        try:
            verify(bad, fixture)
        except (AssertionError, KeyError, StopIteration):
            continue
        raise AssertionError("async verifier accepted corrupted evidence")
    return len(mutations)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claude", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not __debug__:
        raise RuntimeError("Python optimization disables this fixture's assertions")
    fixture = json.loads(FIXTURE.read_text())
    binary = args.claude.resolve(strict=True)
    assert (
        hashlib.sha256(binary.read_bytes()).hexdigest() == fixture["executable_sha256"]
    )
    base = args.output.resolve()
    base.mkdir(mode=0o700)
    captured = {
        "driver.py": Path(__file__),
        "source.py": Path(source.__file__),
        "fixture.json": FIXTURE,
    }
    before = {}
    for name, path in captured.items():
        content = path.read_bytes()
        (base / name).write_bytes(content)
        before[name] = hashlib.sha256(content).hexdigest()
    reports = []
    for name, variant in fixture["cases"].items():
        case = source.run(
            binary,
            base,
            name,
            True,
            variant["exit_code"],
            variant["background"],
            observer=source.ObserverProbe(**variant["observer"]),
        )
        root = Path(case["root"])
        data = load(root)
        verify(data, fixture)
        rejected = attack(data, fixture)
        reports.append(
            {
                "case": name,
                "root": str(root),
                "mutations_rejected": rejected,
                "artifacts": [
                    {
                        "path": name,
                        "sha256": hashlib.sha256(
                            (root / name).read_bytes()
                        ).hexdigest(),
                    }
                    for name in FILES
                ],
            }
        )
    assert (
        hashlib.sha256(binary.read_bytes()).hexdigest() == fixture["executable_sha256"]
    )
    assert all(
        hashlib.sha256(path.read_bytes()).hexdigest() == before[name]
        for name, path in captured.items()
    )
    report = {
        "kind": fixture["kind"],
        "executable_sha256": fixture["executable_sha256"],
        "inputs": before,
        "cases": reports,
        "limits": fixture["limits"],
    }
    (base / "qualification.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        f"PASS: {len(reports)} async source cases, {sum(r['mutations_rejected'] for r in reports)} corrupted-evidence rejections"
    )


if __name__ == "__main__":
    main()
