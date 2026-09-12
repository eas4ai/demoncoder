#!/usr/bin/env python3
"""Assert real Codex compaction effects while faulting a required relay at its wait."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import shlex
import signal
import subprocess
import threading
import time


REPOSITORY = Path(__file__).resolve().parents[2]
REQUIREMENT_ENV = "CODEX_DEMONCODER_COMPACTION_RELAY"
CAPABILITY = {
    "protocol": "demoncoder-compaction-v1",
    "source_version": "0.153.4",
    "patch_version": 1,
}
RELAY = r"""import json, os, sys, time
from pathlib import Path
request = json.load(sys.stdin)
directory = Path(sys.argv[1])
challenge = request["demonCoderCompaction"]["challenge"]
pending = directory / ("request-" + challenge + ".json")
temporary = pending.with_suffix(".tmp")
temporary.write_text(json.dumps({"pid": os.getpid(), "input": request}))
temporary.replace(pending)
response = directory / ("response-" + challenge + ".json")
deadline = time.monotonic() + 10
while not response.exists():
    if time.monotonic() > deadline:
        raise TimeoutError("test owner did not respond")
    time.sleep(0.005)
reply = json.loads(response.read_text())
sys.stdout.write(reply["stdout"])
sys.stdout.flush()
sys.exit(reply["exit_code"])
"""


def require(condition, explanation):
    if not condition:
        raise AssertionError(explanation)


class Backend:
    def __init__(self, process, wire):
        self.process = process
        self.wire = wire
        self.sequence = 0
        self.buffer = b""

    def send(self, method, params, request=True):
        self.sequence += 1
        message = {"method": method, "params": params}
        if request:
            message["id"] = self.sequence
        self.process.stdin.write((json.dumps(message) + "\n").encode())
        self.process.stdin.flush()
        return self.sequence

    def receive(self):
        deadline = time.monotonic() + 25
        while b"\n" not in self.buffer:
            require(
                self.process.poll() is None,
                "backend exited before settling its guarded operation",
            )
            require(
                time.monotonic() < deadline,
                "backend did not settle its guarded operation",
            )
            if select.select([self.process.stdout], [], [], 0.1)[0]:
                chunk = os.read(self.process.stdout.fileno(), 65_536)
                require(bool(chunk), "backend output closed")
                self.buffer += chunk
                require(
                    len(self.buffer) <= 4 * 1024 * 1024,
                    "oversized backend wire message",
                )
        line, self.buffer = self.buffer.split(b"\n", 1)
        message = json.loads(line)
        self.wire.write(json.dumps(message) + "\n")
        self.wire.flush()
        return message

    def rpc(self, method, params):
        identifier = self.send(method, params)
        deadline = time.monotonic() + 30
        while True:
            require(time.monotonic() < deadline, f"{method} deadline expired")
            message = self.receive()
            if message.get("id") == identifier:
                require(
                    "error" not in message, f"{method} failed: {message.get('error')}"
                )
                return message["result"]

    def settle(self):
        deadline = time.monotonic() + 30
        while self.receive().get("method") != "turn/completed":
            require(time.monotonic() < deadline, "turn settlement deadline expired")

    def turn(self, thread):
        self.rpc(
            "turn/start",
            {
                "threadId": thread,
                "input": [{"type": "text", "text": "hello", "text_elements": []}],
            },
        )
        self.settle()


def own_waits(directory, target, mode, stop, seen, errors):
    """Stay alive during the fault, including SIGKILL of the separate relay process."""
    try:
        processed = set()
        while not stop.wait(0.005):
            for path in sorted(directory.glob("request-*.json")):
                if path in processed:
                    continue
                processed.add(path)
                request = json.loads(path.read_text())
                data = request["input"]
                seen.append(data)
                selected = mode if data["hook_event_name"] == target else "allow"
                if selected == "crash":
                    os.kill(request["pid"], signal.SIGKILL)
                    continue
                if selected == "timeout":
                    continue
                ack = dict(data["demonCoderCompaction"])
                ack.update(
                    {
                        key: data[key]
                        for key in ["hook_event_name", "session_id", "turn_id"]
                    }
                )
                if selected == "stale":
                    ack["challenge"] = "previous-operation"
                output = json.dumps(
                    {"continue": selected != "deny", "demonCoderCompaction": ack}
                )
                if selected == "empty":
                    output = ""
                if selected == "plain":
                    output = "completed"
                if selected == "malformed":
                    output = "{invalid"
                response = directory / (
                    "response-" + data["demonCoderCompaction"]["challenge"] + ".json"
                )
                temporary = response.with_suffix(".tmp")
                temporary.write_text(
                    json.dumps(
                        {
                            "stdout": output,
                            "exit_code": 3 if selected == "nonzero" else 0,
                        }
                    )
                )
                temporary.replace(response)
    except BaseException as error:
        errors.append(repr(error))


def case(binary, directory, automatic, target, mode):
    directory.mkdir(parents=True)
    home = directory / "home"
    home.mkdir()
    work = directory / "work"
    work.mkdir()
    subprocess.run(["git", "init", "-q", str(work)], check=True)
    relay_directory = directory / "private-relay"
    relay_directory.mkdir(mode=0o700)
    script = relay_directory / "relay.py"
    script.write_text(RELAY)
    command = shlex.join(["/usr/bin/python3", str(script), str(relay_directory)])
    group = [{"hooks": [{"type": "command", "command": command, "timeout": 1}]}]
    source = relay_directory / "hooks.json"
    source.write_text(
        json.dumps({"hooks": {"PreCompact": group, "PostCompact": group}})
    )
    requirement = {
        **CAPABILITY,
        "source_path": str(source),
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "command": command,
    }
    requirement.pop("source_version")
    requirement.pop("patch_version")
    stop = threading.Event()
    seen, owner_errors = [], []
    owner = threading.Thread(
        target=own_waits,
        args=(relay_directory, target, mode, stop, seen, owner_errors),
        daemon=True,
    )
    owner.start()
    peer = process = peer_errors = None
    result = None
    try:
        peer_errors = (directory / "model-stderr.txt").open("wb")
        peer = subprocess.Popen(
            [
                "/usr/bin/python3",
                str(REPOSITORY / "tests/plugin_codex_model.py"),
                str(directory),
                "auto" if automatic else "manual",
            ],
            stdout=subprocess.PIPE,
            stderr=peer_errors,
        )
        require(
            bool(select.select([peer.stdout], [], [], 10)[0]),
            "model fixture startup timed out",
        )
        ready = json.loads(peer.stdout.readline())
        proxy = f"http://127.0.0.1:{ready['port']}"
        environment = {
            "PATH": "/usr/bin:/bin",
            "HOME": str(home),
            "CODEX_HOME": str(directory / "codex-home"),
            "HTTP_PROXY": proxy,
            "HTTPS_PROXY": proxy,
            "ALL_PROXY": proxy,
            "NO_PROXY": "",
            "CODEX_CA_CERTIFICATE": ready["ca"],
            REQUIREMENT_ENV: json.dumps(requirement),
        }
        with (
            (directory / "backend-stderr.txt").open("wb") as errors,
            (directory / "wire.jsonl").open("w") as wire,
        ):
            process = subprocess.Popen(
                [
                    str(binary),
                    "app-server",
                    "--disable",
                    "hooks",
                    "--disable",
                    "shell_tool",
                    "--disable",
                    "plugins",
                    "--disable",
                    "apps",
                    "-c",
                    'web_search="disabled"',
                ],
                cwd=work,
                env=environment,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=errors,
            )
            backend = Backend(process, wire)
            backend.rpc(
                "initialize",
                {
                    "clientInfo": {
                        "name": "managed-compaction-qualification",
                        "version": "1",
                    },
                    "capabilities": {"experimentalApi": True},
                },
            )
            backend.send("initialized", {}, request=False)
            thread = backend.rpc(
                "thread/start",
                {
                    "model": "gpt-5.4",
                    "cwd": str(work),
                    "approvalPolicy": "never",
                    "sandbox": "danger-full-access",
                    "environments": [],
                },
            )["thread"]["id"]
            backend.turn(thread)
            if automatic:
                backend.turn(thread)
            else:
                backend.rpc("thread/compact/start", {"threadId": thread})
                backend.settle()
            require(
                owner.is_alive() and not owner_errors,
                f"owner died instead of holding the boundary: {owner_errors}",
            )
            require(
                process.poll() is None,
                "backend died instead of settling the guarded operation",
            )
            rows = [
                json.loads(line)
                for line in (directory / "model-requests.jsonl")
                .read_text()
                .splitlines()
            ]
            compacted = sum(row["compact"] for row in rows)
            ordinary = sum(not row["compact"] for row in rows)
            expected_compaction = int(mode == "allow" or target == "PostCompact")
            require(
                compacted == expected_compaction,
                f"unadmitted or missing compaction: {compacted}, expected {expected_compaction}",
            )
            require(
                ordinary == (2 if automatic and mode == "allow" else 1),
                f"unadmitted continuation reached the model: {ordinary}",
            )
            require(
                any(item["hook_event_name"] == target for item in seen),
                "requested fault boundary was never reached",
            )
            require(
                all(
                    item["trigger"] == ("auto" if automatic else "manual")
                    for item in seen
                ),
                "wrong compaction trigger",
            )
            challenges = [item["demonCoderCompaction"]["challenge"] for item in seen]
            require(
                len(challenges) == len(set(challenges)),
                "reused backend operation challenge",
            )
            result = {
                "automatic": automatic,
                "event": target,
                "fault": mode,
                "compactions": compacted,
                "ordinary_requests": ordinary,
                "owner_alive": True,
                "backend_alive": True,
                "passed": True,
            }
    finally:
        stop.set()
        owner.join(timeout=2)
        for child in [process, peer]:
            if child is not None:
                if child.poll() is None:
                    child.kill()
                child.wait(timeout=10)
                for stream in [child.stdin, child.stdout]:
                    if stream is not None:
                        stream.close()
        if peer_errors is not None:
            peer_errors.close()
        (directory / "callbacks.json").write_text(json.dumps(seen, indent=2) + "\n")
        if result is not None:
            (directory / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def startup_cases(binary, directory):
    directory.mkdir(parents=True)
    source = directory / "hooks.json"
    group = [{"hooks": [{"type": "command", "command": "true", "timeout": 1}]}]
    source.write_text(
        json.dumps({"hooks": {"PreCompact": group, "PostCompact": group}})
    )
    expected = {
        "protocol": CAPABILITY["protocol"],
        "source_path": str(source),
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "command": "true",
    }
    cases = {"malformed": "{", "null": "null", "empty": ""}
    for label, field, value in [
        ("wrong-hash", "source_sha256", "0" * 64),
        ("wrong-command", "command", "different"),
        ("wrong-protocol", "protocol", "other"),
        ("missing-source", "source_path", str(directory / "absent.json")),
        ("relative-source", "source_path", "hooks.json"),
        ("disabled-requirement", "enabled", False),
    ]:
        cases[label] = json.dumps({**expected, field: value})
    missing_hash = dict(expected)
    missing_hash.pop("source_sha256")
    cases["missing-hash"] = json.dumps(missing_hash)
    if os.name == "posix":
        fifo = directory / "source-fifo"
        os.mkfifo(fifo)
        source_directory = directory / "source-directory"
        source_directory.mkdir()
        source_link = directory / "source-link"
        source_link.symlink_to(source)
        for label, path in [
            ("fifo", fifo),
            ("directory", source_directory),
            ("symlink", source_link),
        ]:
            cases[label] = json.dumps({**expected, "source_path": str(path)})
    environment = os.environ.copy()
    environment[REQUIREMENT_ENV] = json.dumps(expected)
    valid = subprocess.run(
        [str(binary), "--version"], env=environment, capture_output=True, timeout=10
    )
    require(valid.returncode == 0, "valid startup declaration was rejected")
    results = [{"case": "valid", "passed": True}]
    for label, raw in cases.items():
        environment[REQUIREMENT_ENV] = raw
        result = subprocess.run(
            [str(binary), "--version"],
            env=environment,
            capture_output=True,
            timeout=2 if label in {"fifo", "directory", "symlink"} else 10,
        )
        require(
            result.returncode != 0, f"startup silently dropped required relay: {label}"
        )
        results.append({"case": label, "passed": True})
    for label in ["missing-boundary", "async", "duplicate"]:
        declaration = {"hooks": {"PreCompact": group, "PostCompact": group}}
        if label == "missing-boundary":
            declaration["hooks"].pop("PreCompact")
        elif label == "async":
            declaration["hooks"]["PreCompact"] = [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "true",
                            "timeout": 1,
                            "async": True,
                        }
                    ]
                }
            ]
        else:
            declaration["hooks"]["PreCompact"] = group + group
        source.write_text(json.dumps(declaration))
        environment[REQUIREMENT_ENV] = json.dumps(
            {
                **expected,
                "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            }
        )
        result = subprocess.run(
            [str(binary), "--version"], env=environment, capture_output=True, timeout=10
        )
        require(
            result.returncode != 0,
            f"startup accepted invalid managed declaration: {label}",
        )
        results.append({"case": label, "passed": True})
    (directory / "startup-qualification.json").write_text(
        json.dumps(results, indent=2) + "\n"
    )
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--startup-only", action="store_true")
    parser.add_argument(
        "--mode",
        choices=[
            "allow",
            "deny",
            "crash",
            "timeout",
            "stale",
            "empty",
            "plain",
            "malformed",
            "nonzero",
        ],
    )
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.pop(REQUIREMENT_ENV, None)
    capability = json.loads(
        subprocess.check_output(
            [str(binary), "--demoncoder-compaction-capability"],
            env=environment,
            timeout=10,
        )
    )
    require(capability == CAPABILITY, "binary is not the expected managed integration")
    startup = startup_cases(binary, output / "startup")
    if args.startup_only:
        print(
            json.dumps(
                {
                    "startup": startup,
                    "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                }
            )
        )
        return
    results = []
    for automatic in [False, True]:
        for event in ["PreCompact", "PostCompact"]:
            for mode in (
                [args.mode]
                if args.mode
                else [
                    "allow",
                    "deny",
                    "crash",
                    "timeout",
                    "stale",
                    "empty",
                    "plain",
                    "malformed",
                    "nonzero",
                ]
            ):
                directory = (
                    output / f"{'auto' if automatic else 'manual'}-{event}-{mode}"
                )
                results.append(case(binary, directory, automatic, event, mode))
                print(json.dumps(results[-1]), flush=True)
    receipt = {
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "capability": capability,
        "startup": startup,
        "cases": results,
    }
    (output / "qualification.json").write_text(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
