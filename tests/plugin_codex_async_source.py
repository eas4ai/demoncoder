#!/usr/bin/env python3
"""Qualify pinned Codex async context and idle behavior with synthetic local peers."""

import argparse
import copy
import hashlib
import json
from pathlib import Path

import codex_https_fixture
import installed_backends
import plugin_codex_post_source as source

FIXTURE = Path(__file__).parent / "fixtures/plugins/codex-async-source.json"
MARKER = "CODEX_ASYNC_CONTEXT_MARKER"
FILES = [
    "events.json",
    "model-requests.json",
    "trace.json",
    "hooks.jsonl",
    "hooks.jsonl.times",
    "observation.json",
    "capture.py",
    "codex-home/config.toml",
    "stderr",
]


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def load(root):
    return {
        "events": json.loads((root / "events.json").read_text()),
        "requests": json.loads((root / "model-requests.json").read_text()),
        "trace": json.loads((root / "trace.json").read_text()),
        "hooks": [
            json.loads(line) for line in (root / "hooks.jsonl").read_text().splitlines()
        ],
        "timing": [
            json.loads(line)
            for line in (root / "hooks.jsonl.times").read_text().splitlines()
        ],
        "observation": json.loads((root / "observation.json").read_text()),
        "config": (root / "codex-home/config.toml").read_text(),
        "capture": (root / "capture.py").read_text(),
    }


def verify(data, expected, post_fixture, root):
    trace = data["trace"]
    backend = [row for row in trace if row["direction"] == "backend"]
    host = [row for row in trace if row["direction"] == "host"]
    model = [row for row in trace if row["direction"] == "model"]
    assert [row["message"] for row in backend] == data["events"]
    assert [row["message"] for row in model] == data["requests"]
    assert len(model) == 3
    assert not any("error" in row["message"] for row in backend)

    def response(identifier):
        rows = [
            row["message"]["result"]
            for row in backend
            if row["message"].get("id") == identifier and "result" in row["message"]
        ]
        assert len(rows) == 1
        return rows[0]

    thread = response(3)["thread"]["id"]
    transcript = response(3)["thread"]["path"]
    first_turn = response(4)["turn"]["id"]
    second_turn = response(6)["turn"]["id"]
    assert first_turn != second_turn
    assert data["observation"] == {
        "asynchronous": expected["asynchronous"],
        "exit_code": expected["exit_code"],
        "thread": thread,
        "turn": first_turn,
        "transcript": transcript,
    }
    # A source-supplied path cannot make the fixture read outside its own output.
    Path(transcript).resolve(strict=True).relative_to(root.resolve())
    calls = [
        row["message"]["params"]
        for row in backend
        if row["message"].get("method") == "item/tool/call"
    ]
    source.verify(
        data["hooks"],
        data["requests"][:2],
        calls,
        post_fixture,
        True,
        thread,
        first_turn,
        root / "work",
        transcript,
    )
    starts = [row for row in host if row["message"].get("method") == "turn/start"]
    done = [row for row in backend if row["message"].get("method") == "turn/completed"]
    assert len(starts) == len(done) == 2
    assert [row["message"]["id"] for row in starts] == [4, 6]
    assert all(row["message"]["params"]["threadId"] == thread for row in starts + done)
    assert [row["message"]["params"]["input"] for row in starts] == [
        [{"type": "text", "text": "Perform the fixture operation once."}],
        [{"type": "text", "text": "Continue without another tool."}],
    ]
    assert [row["message"]["params"]["turn"]["id"] for row in done] == [
        first_turn,
        second_turn,
    ]
    assert all(
        row["message"]["params"]["turn"]["status"] == "completed" for row in done
    )
    timing = data["timing"]
    assert [row["stage"] for row in timing] == ["start", "end"]
    assert timing[0]["event"] == timing[1]["event"] == data["hooks"][-1]
    end = timing[1]["time"]
    assert end - timing[0]["time"] >= 0.3
    if expected["asynchronous"]:
        assert model[1]["time"] < end and done[0]["time"] < end
    else:
        assert model[1]["time"] > end
    assert starts[1]["time"] >= done[0]["time"] + 1.9
    assert starts[1]["time"] >= end + 0.5
    assert starts[1]["time"] < model[2]["time"] < done[1]["time"]
    present = [
        any(
            item.get("type") == "message"
            and item.get("role") == "developer"
            and any(
                block.get("type") == "input_text" and MARKER in block.get("text", "")
                for block in item.get("content", [])
            )
            for item in request["input"]
        )
        for request in data["requests"]
    ]
    assert present == expected["context_per_request"]
    assert f",timeout=5,async={str(expected['asynchronous']).lower()}" in data["config"]
    assert "time.sleep(0.4)" in data["capture"]
    assert f"sys.exit({expected['exit_code']})" in data["capture"]


def attack(data, expected, post_fixture, root):
    mutations = []

    def changed(edit):
        bad = copy.deepcopy(data)
        edit(bad)
        mutations.append(bad)

    changed(lambda d: d.update(hooks=[]))
    changed(lambda d: d.update(events=[]))
    changed(lambda d: d.update(trace=[]))
    changed(lambda d: d.update(requests=[]))
    changed(lambda d: d.update(timing=[]))
    changed(lambda d: d.update(config="wrong"))
    changed(lambda d: d.update(capture="wrong"))
    changed(lambda d: d["observation"].update(thread="foreign"))
    changed(lambda d: d["hooks"][-1].update(tool_use_id="foreign"))

    def alter_model(d, index, edit):
        edit(d["requests"][index])
        row = [row for row in d["trace"] if row["direction"] == "model"][index]
        edit(row["message"])

    changed(
        lambda d: alter_model(
            d,
            0,
            lambda r: r["input"].append(
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{"type": "input_text", "text": MARKER}],
                }
            ),
        )
    )

    def reverse_order(d):
        model2 = [row for row in d["trace"] if row["direction"] == "model"][1]
        model2["time"] = d["timing"][1]["time"] + (
            0.01 if expected["asynchronous"] else -0.01
        )

    changed(reverse_order)

    def early_resume(d):
        starts = [
            row
            for row in d["trace"]
            if row["direction"] == "host"
            and row["message"].get("method") == "turn/start"
        ]
        starts[1]["time"] = d["timing"][0]["time"]

    changed(early_resume)
    if any(expected["context_per_request"]):

        def misplaced_context(d):
            def move(request):
                request["input"] = json.loads(
                    json.dumps(request["input"]).replace(MARKER, "removed")
                )
                request["metadata"] = {"unrelated": MARKER}

            for index in range(3):
                alter_model(d, index, move)

        changed(misplaced_context)
    for bad in mutations:
        try:
            verify(bad, expected, post_fixture, root)
        except (AssertionError, KeyError, IndexError, StopIteration):
            continue
        raise AssertionError("Codex async verifier accepted corrupted evidence")
    return len(mutations)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not __debug__:
        raise RuntimeError("Python optimization disables this fixture's assertions")
    fixture = json.loads(FIXTURE.read_text())
    post_fixture = json.loads(source.FIXTURE.read_text())
    assert digest(source.FIXTURE) == fixture["post_fixture_sha256"]
    binary = args.codex.resolve(strict=True)
    assert digest(binary) == fixture["executable_sha256"]
    base = args.output.resolve()
    base.mkdir(mode=0o700)
    inputs = {
        "driver.py": Path(__file__),
        "source.py": Path(source.__file__),
        "codex_https_fixture.py": Path(codex_https_fixture.__file__),
        "installed_backends.py": Path(installed_backends.__file__),
        "fixture.json": FIXTURE,
        "post-fixture.json": source.FIXTURE,
    }
    before = {}
    for name, path in inputs.items():
        (base / name).write_bytes(path.read_bytes())
        before[name] = digest(path)
    reports = []
    for name, variant in fixture["cases"].items():
        root = base / name
        observation = source.run_case(
            binary,
            root,
            post_fixture,
            True,
            observer=source.ObserverProbe(
                variant["asynchronous"], variant["exit_code"]
            ),
        )
        (root / "observation.json").write_text(json.dumps(observation, indent=2) + "\n")
        data = load(root)
        verify(data, variant, post_fixture, root)
        rejected = attack(data, variant, post_fixture, root)
        transcript = (
            Path(observation["transcript"])
            .resolve(strict=True)
            .relative_to(root.resolve())
        )
        reports.append(
            {
                "case": name,
                "root": str(root),
                "mutations_rejected": rejected,
                "artifacts": [
                    {"path": str(path), "sha256": digest(root / path)}
                    for path in [*FILES, transcript]
                ],
            }
        )
    assert digest(binary) == fixture["executable_sha256"]
    assert all(digest(path) == before[name] for name, path in inputs.items())
    (base / "qualification.json").write_text(
        json.dumps(
            {
                "kind": fixture["kind"],
                "executable_sha256": fixture["executable_sha256"],
                "inputs": before,
                "cases": reports,
                "limits": fixture["limits"],
            },
            indent=2,
        )
        + "\n"
    )
    print(
        f"PASS: {len(reports)} Codex async source cases, {sum(row['mutations_rejected'] for row in reports)} corrupted-evidence rejections"
    )


if __name__ == "__main__":
    main()
