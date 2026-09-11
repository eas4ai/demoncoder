#!/usr/bin/env python3
"""Qualify the actual private ordinary Codex boundary against a local model peer."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import sys
import threading
import time

from qualify import Backend, require

REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "tests"))
from codex_https_fixture import create_server  # noqa: E402
from installed_backends import fake_codex_auth  # noqa: E402
from plugin_external_non_tool import digest, peer_handler  # noqa: E402

REQUIREMENT_ENV = "CODEX_DEMONCODER_ORDINARY_RELAY"
CAPABILITY = {
    "protocol": "demoncoder-ordinary-v1",
    "source_version": "0.153.4",
    "patch_version": 1,
}
MARKER = "PRIVATE_ORDINARY_CORRECTION"
CONTEXT_MARKER = "PRIVATE_ORDINARY_CONTEXT_"
BOUNDARY_MODES = ["context-limit", "output-limit"]
EVENTS = ["UserPromptSubmit", "Stop"]
MODES = [
    "allow",
    "deny",
    "empty",
    "malformed",
    "nonzero",
    "exit2",
    "timeout",
    "crash",
    "missing-ack",
    "wrong-delivery",
    "wrong-event",
    "wrong-session",
    "wrong-turn",
    "wrong-protocol",
    "missing-continue",
    "null-continue",
    "unknown-field",
    "duplicate-continue",
]
RELAY = r"""import json, os, sys, time
from pathlib import Path
from uuid import UUID
request = json.load(sys.stdin)
directory = Path(sys.argv[1])
delivery = request["demonCoderOrdinary"]["delivery_id"]
if str(UUID(delivery)) != delivery:
    raise ValueError("noncanonical delivery ID")
pending = directory / ("request-" + delivery + ".json")
temporary = pending.with_suffix(".tmp")
temporary.write_text(json.dumps({"pid": os.getpid(), "input": request}))
temporary.replace(pending)
response = directory / ("response-" + delivery + ".json")
deadline = time.monotonic() + 10
while not response.exists():
    if time.monotonic() > deadline:
        raise TimeoutError("qualification owner did not respond")
    time.sleep(0.005)
reply = json.loads(response.read_text())
sys.stdout.write(reply["stdout"])
sys.stdout.flush()
sys.exit(reply["exit_code"])
"""


def input_manifest(binary):
    paths = set()
    for module in tuple(sys.modules.values()):
        filename = getattr(module, "__file__", None)
        if filename:
            path = Path(filename).resolve()
            if path.is_relative_to(REPOSITORY):
                paths.add(path)
    return {
        "binary_sha256": digest(binary),
        "helpers": {
            str(path.relative_to(REPOSITORY)): digest(path) for path in sorted(paths)
        },
        "python": sys.version,
    }


def verify_inputs(binary, expected):
    require(
        digest(binary) == expected["binary_sha256"],
        "backend changed during qualification",
    )
    for name, value in expected["helpers"].items():
        require(
            digest(REPOSITORY / name) == value, f"qualification helper changed: {name}"
        )


def reply_for(data, mode):
    acknowledgment = dict(data["demonCoderOrdinary"])
    output = {"continue": mode != "deny", "demonCoderOrdinary": acknowledgment}
    if mode == "deny":
        output["stopReason"] = "PRIVATE_ORDINARY_DENY"
    if mode == "correction" or (
        data["hook_event_name"] == "Stop" and mode not in {"allow", "deny"}
    ):
        output.update({"decision": "block", "reason": MARKER})
    replacements = {
        "wrong-delivery": ("delivery_id", "previous-delivery"),
        "wrong-event": ("hook_event_name", "PreCompact"),
        "wrong-session": ("session_id", "other-session"),
        "wrong-turn": ("turn_id", "other-turn"),
        "wrong-protocol": ("protocol", "demoncoder-compaction-v1"),
    }
    if mode in replacements:
        field, value = replacements[mode]
        acknowledgment[field] = value
    if mode == "missing-ack":
        output.pop("demonCoderOrdinary")
    if mode == "missing-continue":
        output.pop("continue")
    if mode == "null-continue":
        output["continue"] = None
    if mode == "unknown-field":
        output["unrecognized"] = True
    if mode in BOUNDARY_MODES:
        context = {"hookEventName": "UserPromptSubmit", "additionalContext": ""}
        output["hookSpecificOutput"] = context
        target_bytes = 65536 if mode == "context-limit" else 65537
        remaining = target_bytes - len(json.dumps(output).encode())
        context["additionalContext"] = CONTEXT_MARKER + "x" * (
            remaining - len(CONTEXT_MARKER)
        )
    raw = json.dumps(output)
    if mode in BOUNDARY_MODES:
        require(len(raw.encode()) == target_bytes, "boundary response size differs")
    if mode == "empty":
        raw = ""
    if mode == "malformed":
        raw = "{invalid"
    if mode == "duplicate-continue":
        raw = '{"continue":false,' + raw[1:]
    return {
        "stdout": raw,
        "exit_code": 1 if mode == "nonzero" else 2 if mode == "exit2" else 0,
    }


def own_waits(directory, target, mode, stop, seen, errors):
    """The owner survives relay faults so killing it cannot establish safety."""
    try:
        processed = set()
        corrected = False
        while not stop.wait(0.005):
            for path in sorted(directory.glob("request-*.json")):
                if path in processed:
                    continue
                require(
                    len(processed) < 8, "callback count exceeded qualification bound"
                )
                require(
                    path.stat().st_size <= 2 * 1024 * 1024, "oversized callback capture"
                )
                processed.add(path)
                request = json.loads(path.read_text())
                data = request["input"]
                seen.append(data)
                selected = mode if data["hook_event_name"] == target else "allow"
                if selected == "correction":
                    if corrected:
                        selected = "allow"
                    corrected = True
                if selected == "timeout":
                    continue
                if selected == "crash":
                    descriptor = os.pidfd_open(request["pid"])
                    try:
                        command = Path(f"/proc/{request['pid']}/cmdline").read_bytes()
                        require(
                            str(directory / "relay.py").encode()
                            in command.split(b"\0"),
                            "relay PID does not name the owned script",
                        )
                        signal.pidfd_send_signal(descriptor, signal.SIGKILL)
                    finally:
                        os.close(descriptor)
                    continue
                response = directory / (
                    "response-" + data["demonCoderOrdinary"]["delivery_id"] + ".json"
                )
                temporary = response.with_suffix(".tmp")
                temporary.write_text(json.dumps(reply_for(data, selected)))
                temporary.replace(response)
    except Exception as error:
        errors.append(repr(error))


def declaration(directory, timeout=1):
    directory.mkdir(mode=0o700)
    script = directory / "relay.py"
    script.write_text(RELAY)
    command = shlex.join(["/usr/bin/python3", "-B", str(script), str(directory)])
    group = [
        {
            "hooks": [
                {
                    "type": "command",
                    "command": command,
                    "timeout": timeout,
                    "async": False,
                }
            ]
        }
    ]
    source = directory / "hooks.json"
    source.write_text(json.dumps({"hooks": {event: group for event in EVENTS}}))
    return source, {
        "protocol": CAPABILITY["protocol"],
        "source_path": str(source),
        "source_sha256": digest(source),
        "submit_command": command,
        "stop_command": command,
    }


def startup_cases(binary, root):
    root.mkdir()
    source, valid = declaration(root / "private")
    original = source.read_bytes()
    variants = [("valid", json.dumps(valid), original, True)]
    for name, raw in [("malformed", "{"), ("null", "null"), ("empty", "")]:
        variants.append((name, raw, original, False))
    for name, field, value in [
        ("wrong-hash", "source_sha256", "0" * 64),
        ("uppercase-hash", "source_sha256", valid["source_sha256"].upper()),
        ("wrong-protocol", "protocol", "demoncoder-compaction-v1"),
        ("wrong-submit-command", "submit_command", "different"),
        ("wrong-stop-command", "stop_command", "different"),
        ("relative-source", "source_path", "hooks.json"),
        ("missing-source", "source_path", str(root / "absent")),
        ("unknown-requirement-field", "enabled", False),
    ]:
        variants.append((name, json.dumps({**valid, field: value}), original, False))
    missing = dict(valid)
    missing.pop("source_sha256")
    variants.append(("missing-hash", json.dumps(missing), original, False))
    variants.append(
        (
            "duplicate-requirement-field",
            '{"protocol":"other",' + json.dumps(valid)[1:],
            original,
            False,
        )
    )
    fifo = root / "fifo"
    os.mkfifo(fifo)
    link = root / "source-link"
    link.symlink_to(source)
    for name, path in [("fifo", fifo), ("directory", root), ("symlink", link)]:
        variants.append(
            (name, json.dumps({**valid, "source_path": str(path)}), original, False)
        )
    for name in [
        "missing-event",
        "duplicate-group",
        "async",
        "unknown-hook-field",
        "wrong-type",
        "zero-timeout",
        "oversized-timeout",
        "duplicate-json-key",
    ]:
        document = json.loads(original)
        hook = document["hooks"]["UserPromptSubmit"][0]["hooks"][0]
        if name == "missing-event":
            document["hooks"].pop("Stop")
        elif name == "duplicate-group":
            document["hooks"]["Stop"] *= 2
        elif name == "async":
            hook["async"] = True
        elif name == "unknown-hook-field":
            hook["unknown"] = True
        elif name == "wrong-type":
            hook["type"] = "http"
        elif name == "zero-timeout":
            hook["timeout"] = 0
        elif name == "oversized-timeout":
            hook["timeout"] = 121
        raw = json.dumps(document)
        if name == "duplicate-json-key":
            raw = '{"hooks":{},' + raw[1:]
        content = raw.encode()
        requirement = {**valid, "source_sha256": hashlib.sha256(content).hexdigest()}
        variants.append((name, json.dumps(requirement), content, False))
    results = []
    environment = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(root),
        "CODEX_HOME": str(root / "home"),
    }
    for name, raw, content, accepted in variants:
        source.write_bytes(content)
        environment[REQUIREMENT_ENV] = raw
        result = subprocess.run(
            [str(binary), "--version"],
            env=environment,
            capture_output=True,
            timeout=2 if name in {"fifo", "directory", "symlink"} else 10,
        )
        (root / f"{name}.stdout").write_bytes(result.stdout)
        (root / f"{name}.stderr").write_bytes(result.stderr)
        require(
            (result.returncode == 0) == accepted, f"startup acceptance differs: {name}"
        )
        results.append(
            {
                "case": name,
                "exit": result.returncode,
                "accepted": accepted,
                "passed": True,
            }
        )
    require(
        not list((root / "private").glob("request-*.json")),
        "startup executed a hook while inspecting its declaration",
    )
    (root / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    return results


def ambient_configuration(home, work, root):
    script = root / "ambient.py"
    script.write_text(
        "import sys\nfrom pathlib import Path\n"
        'with Path(sys.argv[1]).open("a") as output:\n'
        '    output.write(sys.argv[2] + "\\n")\n'
        "print('{\"continue\":true}', flush=True)\n"
    )
    prefix = ["/usr/bin/python3", "-B", str(script), str(root / "ambient.jsonl")]
    config = (
        'model="gpt-5.4"\ncli_auth_credentials_store="file"\n'
        + "notify="
        + json.dumps(prefix + ["notify"])
        + "\n"
        + "[features]\nenable_request_compression=false\nhooks=true\n"
    )
    command = shlex.join(prefix + ["toml"])
    for event in EVENTS:
        config += (
            "[[hooks."
            + event
            + ']]\nhooks=[{type="command",command='
            + json.dumps(command)
            + ",timeout=2}]\n"
        )
    config += "[projects." + json.dumps(str(work)) + ']\ntrust_level="trusted"\n'
    project = work / ".codex"
    project.mkdir()
    (project / "config.toml").write_text("[features]\nhooks=true\n")
    for folder, tag in [(home, "json"), (project, "workspace")]:
        group = [
            {
                "hooks": [
                    {
                        "type": "command",
                        "command": shlex.join(prefix + [tag]),
                        "timeout": 2,
                    }
                ]
            }
        ]
        (folder / "hooks.json").write_text(
            json.dumps({"hooks": {event: group for event in EVENTS}})
        )
    return config


def case(binary, root, target, mode):
    snapshot = mode == "snapshot"
    ambient = mode in {"ambient-control", "isolation"}
    private = mode != "ambient-control"
    response_mode = "allow" if snapshot or ambient else mode
    root.mkdir(parents=True, exist_ok=False)
    home, work = root / "home", root / "work"
    home.mkdir()
    work.mkdir()
    fake_codex_auth(home)
    config = (
        ambient_configuration(home, work, root)
        if ambient
        else 'model="gpt-5.4"\ncli_auth_credentials_store="file"\n[features]\nenable_request_compression=false\nhooks=false\n'
    )
    (home / "config.toml").write_text(config)
    source, requirement = declaration(root / "private")
    requests, peer_errors, lock = [], [], threading.Lock()
    server = create_server(
        root / "tls",
        peer_handler("codex", requests, peer_errors, lock, "plain", "pass"),
    )
    server.daemon_threads = True
    peer = threading.Thread(target=server.serve_forever, daemon=True)
    peer.start()
    stop, seen, owner_errors = threading.Event(), [], []
    owner = threading.Thread(
        target=own_waits,
        args=(root / "private", target, response_mode, stop, seen, owner_errors),
        daemon=True,
    )
    owner.start()
    environment = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(home),
        "CODEX_HOME": str(home),
        "CODEX_CA_CERTIFICATE": str(server.ca_certificate),
        "NO_PROXY": "",
        REQUIREMENT_ENV: json.dumps(requirement),
    }
    for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"]:
        environment[key] = f"http://127.0.0.1:{server.server_port}"
    if not private:
        environment.pop(REQUIREMENT_ENV)
    command = [
        str(binary),
        "app-server",
        "--stdio",
        "--enable" if ambient else "--disable",
        "hooks",
        "--disable",
        "plugins",
        "--disable",
        "shell_tool",
        "--disable",
        "apps",
        "-c",
        'web_search="disabled"',
    ]
    (root / "inputs.json").write_text(
        json.dumps(
            {
                "binary_sha256": digest(binary),
                "launcher_sha256": digest(Path(__file__)),
                "command": command,
                "requirement": requirement if private else None,
                "declaration_text": source.read_text(),
                "config_text": config,
                "private": private,
                "ambient": ambient,
                "target": target,
                "mode": mode,
                "snapshot": snapshot,
            },
            indent=2,
        )
        + "\n"
    )
    process = None
    turns = []
    try:
        with (root / "stderr.log").open("wb") as stderr, (root / "wire.jsonl").open(
            "w"
        ) as wire:
            process = subprocess.Popen(
                command,
                cwd=work,
                env=environment,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=stderr,
                start_new_session=True,
            )
            backend = Backend(process, wire)
            backend.rpc(
                "initialize",
                {
                    "clientInfo": {
                        "name": "private-ordinary-qualification",
                        "version": "1",
                    },
                    "capabilities": {"experimentalApi": True},
                },
            )
            backend.send("initialized", {}, request=False)
            if snapshot:
                source.write_text("invalid after startup\n")
            for _ in range(2 if snapshot else 1):
                thread = backend.rpc(
                    "thread/start",
                    {
                        "model": "gpt-5.4",
                        "cwd": str(work),
                        "approvalPolicy": "never",
                        "sandbox": "danger-full-access",
                        "environments": [],
                        # Native command hooks require their own approval. Make
                        # the same harmless canaries eligible in both controls.
                        **({"config": {"bypass_hook_trust": True}} if ambient else {}),
                    },
                )["thread"]["id"]
                result = backend.rpc(
                    "turn/start",
                    {
                        "threadId": thread,
                        "input": [
                            {
                                "type": "text",
                                "text": "Complete this fixture turn.",
                                "text_elements": [],
                            }
                        ],
                    },
                )
                turns.append((thread, result["turn"]["id"]))
                backend.settle()
            require(
                process.poll() is None,
                "backend died instead of holding/settling ordinary work",
            )
            require(
                owner.is_alive() and not owner_errors,
                f"relay owner failed: {owner_errors}",
            )
            if ambient and not private:
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline:
                    canary = root / "ambient.jsonl"
                    tags = (
                        set(canary.read_text().splitlines())
                        if canary.exists()
                        else set()
                    )
                    if {"toml", "json", "workspace", "notify"} <= tags:
                        break
                    time.sleep(0.005)
    finally:
        stop.set()
        owner.join(timeout=2)
        if process is not None:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
            for stream in [process.stdin, process.stdout]:
                if stream is not None:
                    stream.close()
        server.shutdown()
        server.server_close()
        peer.join(timeout=2)
        (root / "callbacks.json").write_text(json.dumps(seen, indent=2) + "\n")
        (root / "model-requests.json").write_text(json.dumps(requests, indent=2) + "\n")
        (root / "errors.json").write_text(
            json.dumps(owner_errors + peer_errors + server.errors, indent=2) + "\n"
        )
    require(
        not owner_errors and not peer_errors and not server.errors,
        "qualification peer/owner failed",
    )
    expected_models = (
        2
        if snapshot or mode == "correction"
        else 1 if response_mode in {"allow", "context-limit"} or target == "Stop" else 0
    )
    require(
        len(requests) == expected_models,
        f"model requests {len(requests)} != {expected_models}",
    )
    expected_events = (
        []
        if not private
        else (
            EVENTS * 2
            if snapshot
            else (
                [*EVENTS, "Stop"]
                if mode == "correction"
                else EVENTS if expected_models else ["UserPromptSubmit"]
            )
        )
    )
    require(
        [item["hook_event_name"] for item in seen] == expected_events,
        "actual source callback sequence differs",
    )
    if ambient:
        canary = root / "ambient.jsonl"
        tags = set(canary.read_text().splitlines()) if canary.exists() else set()
        require(
            not tags if private else {"toml", "json", "workspace", "notify"} <= tags,
            f"ambient execution differs: {sorted(tags)}",
        )
    deliveries = set()
    for item in seen:
        acknowledgment = item["demonCoderOrdinary"]
        require(
            (item["session_id"], item["turn_id"]) in turns,
            "callback does not belong to an actual source turn",
        )
        require(
            acknowledgment
            == {
                "protocol": CAPABILITY["protocol"],
                "delivery_id": acknowledgment["delivery_id"],
                "hook_event_name": item["hook_event_name"],
                "session_id": item["session_id"],
                "turn_id": item["turn_id"],
            },
            "private envelope changes native source identity",
        )
        require(
            acknowledgment["delivery_id"] not in deliveries,
            "transport delivery ID repeated",
        )
        deliveries.add(acknowledgment["delivery_id"])
        require(item["cwd"] == str(work), "source workspace differs")
    if mode == "correction":
        require(
            MARKER not in json.dumps(requests[0]["body"]),
            "correction leaked before source response",
        )
        require(
            MARKER in json.dumps(requests[1]["body"]),
            "actual correction did not reach the next model request",
        )
    if mode == "context-limit":
        require(
            CONTEXT_MARKER in json.dumps(requests[0]["body"]),
            "accepted source context did not reach model",
        )
    result = {
        "target": target,
        "mode": mode,
        "snapshot": snapshot,
        "model_requests": len(requests),
        "callbacks": len(seen),
        "passed": True,
    }
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--mode",
        choices=MODES
        + BOUNDARY_MODES
        + ["correction", "snapshot", "ambient-control", "isolation"],
    )
    parser.add_argument("--event", choices=EVENTS)
    parser.add_argument("--startup-only", action="store_true")
    args = parser.parse_args()
    if args.mode == "correction" and args.event == "UserPromptSubmit":
        parser.error("correction requires ordinary Stop")
    if args.mode in BOUNDARY_MODES and args.event == "Stop":
        parser.error("context boundary cases require UserPromptSubmit")
    binary = args.binary.resolve(strict=True)
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    frozen = input_manifest(binary)
    (root / "inputs.json").write_text(json.dumps(frozen, indent=2) + "\n")
    environment = os.environ.copy()
    environment.pop(REQUIREMENT_ENV, None)
    environment.pop("CODEX_DEMONCODER_COMPACTION_RELAY", None)
    capability = json.loads(
        subprocess.check_output(
            [str(binary), "--demoncoder-ordinary-capability"],
            env=environment,
            timeout=10,
        )
    )
    require(
        capability == CAPABILITY, "artifact lacks the exact private ordinary capability"
    )
    startup = startup_cases(binary, root / "startup")
    verify_inputs(binary, frozen)
    if args.startup_only:
        print(
            json.dumps({"startup_cases": len(startup), "binary_sha256": digest(binary)})
        )
        return
    cases = [
        (event, mode)
        for event in (EVENTS if args.event is None else [args.event])
        for mode in (MODES if args.mode is None else [args.mode])
        if mode != "correction" or event == "Stop"
        if mode not in BOUNDARY_MODES or event == "UserPromptSubmit"
    ]
    if args.mode is None:
        cases += [
            ("Stop", "correction"),
            ("UserPromptSubmit", "snapshot"),
            ("UserPromptSubmit", "ambient-control"),
            ("UserPromptSubmit", "isolation"),
            ("UserPromptSubmit", "context-limit"),
            ("UserPromptSubmit", "output-limit"),
        ]
    results = []
    for event, mode in cases:
        results.append(
            case(
                binary,
                root / f"{event}-{mode}",
                event,
                mode,
            )
        )
        verify_inputs(binary, frozen)
        print(json.dumps(results[-1]), flush=True)
    (root / "qualification.json").write_text(
        json.dumps(
            {
                "binary_sha256": digest(binary),
                "capability": capability,
                "startup": startup,
                "cases": results,
                "kind": "actual-source-controlled-model-peer",
            },
            indent=2,
        )
        + "\n"
    )


if __name__ == "__main__":
    main()
