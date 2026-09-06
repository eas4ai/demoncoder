#!/usr/bin/env python3
"""Run, retain, and verify live two-turn sessions without retaining credentials."""
import argparse
import ast
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import secrets
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import time

sys.dont_write_bytecode = True
from terminal_session import BINARY, ROOT, until

ADAPTERS = ("openai-api", "anthropic-api", "codex", "claude")
INPUTS = ["Cargo.toml", "Cargo.lock", "build.rs", "src", "tests/live_connections.py", "tests/terminal_session.py", "tests/tool_cycle_fixture.py", "docs/spec", "docs/commitments/first-coding-session.md"]
EVIDENCE = ROOT / ".cairn/evidence/live"


def input_digest():
    subprocess.run(["git", "diff", "--quiet", "HEAD", "--", *INPUTS], cwd=ROOT, check=True)
    tree = subprocess.check_output(["git", "ls-tree", "-r", "-z", "HEAD", "--", *INPUTS], cwd=ROOT)
    return hashlib.sha256(tree).hexdigest()


def function(source, expected):
    tree = ast.parse(source)
    assert len(tree.body) == 1 and isinstance(tree.body[0], ast.FunctionDef), "expected one function"
    node = tree.body[0]
    assert node.name.startswith("dc_") and not node.decorator_list
    assert not node.args.args and not node.args.posonlyargs and not node.args.kwonlyargs
    assert node.args.vararg is None and node.args.kwarg is None
    assert len(node.body) == 1 and isinstance(node.body[0], ast.Return)
    value = node.body[0].value
    assert isinstance(value, ast.Constant) and type(value.value) is int and value.value == expected
    return node.name


def validate(record, digest):
    assert record["input_digest"] == digest, "live evidence is stale"
    assert record["adapter"] in ADAPTERS and record["transport"] == "live-default-endpoint"
    assert record["auth_method"] == ("api-key" if record["adapter"].endswith("-api") else "subscription")
    assert len(record["turns"]) == 2
    name = None
    for index, turn in enumerate(record["turns"]):
        current = function(turn["source"], record["seed"] + (1 if index == 0 else 8))
        assert name is None or current == name, "second turn replaced the first function"
        name = current
        events = turn["events"]
        assert all(row["connection"] == record["adapter"] for row in events)
        events = [row["event"] for row in events]
        endings = [event for event in events if event["type"] == "turn_finished"]
        assert len(endings) == 1 and endings[0]["status"] == "complete"
        calls = {event["call"]["id"]: event["call"] for event in events if event["type"] == "tool_started"}
        results = [event["result"] for event in events if event["type"] == "tool_finished"]
        successful = {result["tool"] for result in results if result["success"]}
        assert ({"read", "write", "edit", "bash"} if index == 0 else {"read", "edit", "bash"}) <= successful
        for result in results:
            assert result["call_id"] in calls, "result has no corresponding request"
            assert result["tool"] == calls[result["call_id"]]["name"]
        checks = [result for result in results if result["tool"] == "bash" and result["success"] and result["exit_code"] == 0]
        assert any("assert" in calls[result["call_id"]]["arguments"]["command"] and "python3" in calls[result["call_id"]]["arguments"]["command"] for result in checks), "no completed Python assertion"
    return name


def records(path):
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith("\n")]


def wait_turn(master, process, output, log, count):
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        assert process.poll() is None, "application exited before completing the live turn"
        if select.select([master], [], [], .05)[0]:
            output.extend(os.read(master, 65536))
        rows = records(log)
        endings = [row["event"] for row in rows if row["event"]["type"] == "turn_finished"]
        if len(endings) >= count:
            assert endings[-1]["status"] == "complete", "live turn failed; redacted record retains the runtime error"
            return rows
    raise AssertionError("live turn exceeded the 180-second smoke-test limit")


def redact(value):
    text = json.dumps(value)
    for name in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"):
        secret = os.environ.get(name)
        if secret:
            text = text.replace(secret, "[REDACTED]")
    return json.loads(text)


def run(adapter, model):
    digest = input_digest()
    key = {"openai-api": "OPENAI_API_KEY", "anthropic-api": "ANTHROPIC_API_KEY"}.get(adapter)
    assert key is None or os.environ.get(key), f"prerequisite missing: {key}"
    if key:
        assert model, "select --model for an API smoke session"
    else:
        assert shutil.which(adapter), f"prerequisite missing: {adapter} executable"
    subprocess.run(["cargo", "build", "--locked"], cwd=ROOT, check=True)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    record = {"adapter": adapter, "model": model or "backend-default", "transport": "live-default-endpoint", "auth_method": "api-key" if key else "subscription", "input_digest": digest, "binary_sha256": hashlib.sha256(BINARY.read_bytes()).hexdigest(), "started_at": stamp, "turns": []}
    if not key:
        record["backend_version"] = subprocess.check_output([adapter, "--version"], text=True).strip()
    path = EVIDENCE / adapter / (stamp + ".json")
    with tempfile.TemporaryDirectory(prefix="demoncoder-live-") as directory:
        workspace = Path(directory) / "workspace"
        workspace.mkdir()
        subprocess.run(["git", "init", "-q", str(workspace)], check=True)
        seed = secrets.randbelow(1000000) + 1000000
        (workspace / "seed.txt").write_text(str(seed) + "\n")
        record["seed"] = seed
        log = Path(directory) / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
        command = [str(BINARY), "--workspace", str(workspace), "--connection", adapter, "--event-log", str(log)]
        if model:
            command.extend(["--model", model])
        env = os.environ.copy()
        env.update(TERM="xterm-256color", LANG="C.UTF-8")
        process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        try:
            until(master, process, output, b"Prompt")
            prompts = [
                "This is a bounded smoke test in a disposable repository. Use the four host tools read, write, edit, and bash. Read seed.txt. Choose a function name beginning dc_. Write answer.py with exactly that one zero-argument function and a single return statement containing the seed as an integer literal. Then use edit to change that literal to seed plus one. Use bash to run python3 -B -c with a real assertion that the function returns the expected value. Do not create other files, add docstrings, use imports in answer.py, or use command wrappers such as rtk. Stop after the check passes.",
                "Continue the same function in answer.py. Read it, then use edit to make it return seven more than it currently does. Keep its name and the single integer-literal return statement. Use bash with python3 -B -c and a real assertion to verify the new value. Stop after that check passes.",
            ]
            offset = 0
            for index, prompt in enumerate(prompts):
                os.write(master, prompt.encode() + b"\r")
                rows = wait_turn(master, process, output, log, index + 1)
                record["turns"].append({"events": rows[offset:], "source": (workspace / "answer.py").read_text()})
                offset = len(rows)
                print(adapter, "live turn", index + 1, "completed", flush=True)
            validate(record, digest)
            record["result"] = "pass"
        except (AssertionError, OSError, ValueError) as error:
            record["result"] = "unverified"
            record["reason"] = str(error)
            record["observed_events"] = records(log)
            raise
        finally:
            os.write(master, b"\x11")
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            os.close(master)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(redact(record), indent=2) + "\n")
            print("retained", path.relative_to(ROOT), flush=True)


def check():
    digest = input_digest()
    missing = []
    for adapter in ADAPTERS:
        paths = sorted((EVIDENCE / adapter).glob("*.json"))
        try:
            assert paths, "no live record"
            record = json.loads(paths[-1].read_text())
            assert record.get("result") == "pass", record.get("reason", "live result is unverified")
            validate(record, digest)
            print("CONN-001", adapter, "current live two-turn record passed")
        except (AssertionError, KeyError, ValueError) as error:
            missing.append(adapter)
            print("CONN-001", adapter, "unverified:", error)
    if not missing:
        print("cairn: CONN-001: pass")
    return int(bool(missing))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", choices=ADAPTERS)
    parser.add_argument("--model")
    args = parser.parse_args()
    if args.run:
        run(args.run, args.model)
        return 0
    return check()


if __name__ == "__main__":
    raise SystemExit(main())
