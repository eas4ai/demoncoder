#!/usr/bin/env python3
"""Exercise actual private Codex callback interruption and same-thread refresh."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import threading
import time

import qualify_ordinary as ordinary
from qualify import Backend, require


class TrackedBackend(Backend):
    def __init__(self, process, wire):
        super().__init__(process, wire)
        self.messages = []

    def receive(self):
        message = super().receive()
        self.messages.append(message)
        return message

    def completed(self, thread, turn):
        for message in self.messages:
            params = message.get("params", {})
            if (
                message.get("method") == "turn/completed"
                and params.get("threadId") == thread
                and params.get("turn", {}).get("id") == turn
            ):
                return params["turn"]
        return None

    def settle_turn(self, thread, turn):
        deadline = time.monotonic() + 30
        while self.completed(thread, turn) is None:
            require(time.monotonic() < deadline, "exact turn did not settle")
            self.receive()
        return self.completed(thread, turn)


def reply(directory, request, mode="allow"):
    data = request["input"]
    path = directory / (
        "response-" + data["demonCoderOrdinary"]["delivery_id"] + ".json"
    )
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(ordinary.reply_for(data, mode)))
    temporary.replace(path)


class Owner:
    def __init__(self, directory, target):
        self.directory, self.target = directory, target
        self.requests, self.errors = [], []
        self.stop, self.waiting = threading.Event(), threading.Event()
        self.held = None
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self):
        processed = set()
        try:
            while not self.stop.wait(0.005):
                for path in sorted(self.directory.glob("request-*.json")):
                    if path in processed:
                        continue
                    require(len(processed) < 8, "excess callback deliveries")
                    require(path.stat().st_size <= 2 * 1024 * 1024, "oversize capture")
                    processed.add(path)
                    request = json.loads(path.read_text())
                    self.requests.append(request)
                    if (
                        request["input"]["hook_event_name"] == self.target
                        and self.held is None
                    ):
                        self.held = request
                        self.waiting.set()
                    else:
                        reply(self.directory, request)
        except Exception as error:
            self.errors.append(repr(error))


@contextmanager
def session(binary, root, target):
    root.mkdir(parents=True, exist_ok=False)
    home, work = root / "home", root / "work"
    home.mkdir()
    work.mkdir()
    ordinary.fake_codex_auth(home)
    (home / "config.toml").write_text(
        'model="gpt-5.4"\ncli_auth_credentials_store="file"\n'
        "[features]\nenable_request_compression=false\nhooks=false\n"
    )
    declaration, requirement = ordinary.declaration(root / "private", timeout=30)
    requests, errors, lock = [], [], threading.Lock()
    server = ordinary.create_server(
        root / "tls",
        ordinary.peer_handler("codex", requests, errors, lock, "plain", "pass"),
    )
    server.daemon_threads = True
    peer = threading.Thread(target=server.serve_forever, daemon=True)
    peer.start()
    owner = Owner(root / "private", target)
    owner.thread.start()
    environment = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(home),
        "CODEX_HOME": str(home),
        "CODEX_CA_CERTIFICATE": str(server.ca_certificate),
        "NO_PROXY": "",
        ordinary.REQUIREMENT_ENV: json.dumps(requirement),
    }
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"]:
        environment[name] = f"http://127.0.0.1:{server.server_port}"
    command = [
        str(binary),
        "app-server",
        "--stdio",
        "--disable",
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
                "command": command,
                "requirement": requirement,
                "declaration": declaration.read_text(),
                "target": target,
            },
            indent=2,
        )
        + "\n"
    )
    process = None
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
            backend = TrackedBackend(process, wire)
            backend.rpc(
                "initialize",
                {
                    "clientInfo": {
                        "name": "ordinary-lifetime-qualification",
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
            yield backend, thread, declaration, owner, requests
            require(process.poll() is None, "backend owner exited")
            require(
                owner.thread.is_alive() and not owner.errors,
                f"relay owner failed: {owner.errors}",
            )
            require(not errors, f"model peer failed: {errors}")
    finally:
        owner.stop.set()
        owner.thread.join(timeout=2)
        if process is not None:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
            for stream in [process.stdin, process.stdout]:
                if stream:
                    stream.close()
        server.shutdown()
        server.server_close()
        peer.join(timeout=2)
        for name, value in [
            ("callbacks", owner.requests),
            ("model-requests", requests),
            ("owner-errors", owner.errors),
            ("peer-errors", errors),
        ]:
            (root / f"{name}.json").write_text(json.dumps(value, indent=2) + "\n")


def start_turn(backend, thread):
    return backend.rpc(
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
    )["turn"]["id"]


def check_identities(owner, thread, turns):
    deliveries = set()
    for request in owner.requests:
        data = request["input"]
        envelope = data["demonCoderOrdinary"]
        require(
            data["session_id"] == thread and data["turn_id"] in turns,
            "wrong native identity",
        )
        for key in ["session_id", "turn_id", "hook_event_name"]:
            require(envelope[key] == data[key], f"wrong envelope {key}")
        require(envelope["delivery_id"] not in deliveries, "reused delivery identity")
        deliveries.add(envelope["delivery_id"])


def cancellation(binary, root, target):
    with session(binary, root, target) as (backend, thread, _, owner, requests):
        turn = start_turn(backend, thread)
        require(owner.waiting.wait(20), "target callback never entered its wait")
        require(not owner.errors, f"owner failed: {owner.errors}")
        held = owner.held
        descriptor = os.pidfd_open(held["pid"])
        try:
            command = Path(f"/proc/{held['pid']}/cmdline").read_bytes().split(b"\0")
            require(
                str(root / "private" / "relay.py").encode() in command,
                "unowned callback PID",
            )
            started = time.monotonic()
            backend.rpc("turn/interrupt", {"threadId": thread, "turnId": turn})
            completed = backend.settle_turn(thread, turn)
            elapsed = time.monotonic() - started
            require(
                completed["status"] == "interrupted",
                f"turn not interrupted: {completed}",
            )
            require(
                select.select([descriptor], [], [], 2)[0],
                "interrupted callback still alive",
            )
            require(elapsed < 5, "interrupt settled only after callback timeout")
        finally:
            os.close(descriptor)
        expected_before = 0 if target == "UserPromptSubmit" else 1
        require(
            len(requests) == expected_before, "cancelled boundary continued model work"
        )
        # A late response to the dead callback must not release the next turn.
        reply(root / "private", held, "correction" if target == "Stop" else "allow")
        next_turn = start_turn(backend, thread)
        completed = backend.settle_turn(thread, next_turn)
        require(
            completed["status"] == "completed", "fresh turn failed after interruption"
        )
        require(
            len(requests) == expected_before + 1, "late reply caused extra model work"
        )
        require(
            ordinary.MARKER not in json.dumps(requests),
            "late Stop correction reached model",
        )
        check_identities(owner, thread, {turn, next_turn})
        result = {
            "event": target,
            "interrupt_seconds": elapsed,
            "model_requests": len(requests),
            "callbacks": len(owner.requests),
            "passed": True,
        }
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def refresh(binary, root):
    with session(binary, root, None) as (backend, thread, declaration, owner, requests):
        first = start_turn(backend, thread)
        require(
            backend.settle_turn(thread, first)["status"] == "completed",
            "first turn failed",
        )
        declaration.write_text("invalid after first turn\n")
        state = {"qualification-refresh-probe": {"enabled": False}}
        parameters = {
            "edits": [
                {"keyPath": "hooks.state", "value": state, "mergeStrategy": "upsert"}
            ],
            "reloadUserConfig": True,
        }
        response = backend.rpc("config/batchWrite", parameters)
        require(response.get("status") == "ok", "configuration write was not applied")
        observed = backend.rpc(
            "config/read", {"includeLayers": True, "cwd": str(root / "work")}
        )
        require(
            observed["config"]["hooks"]["state"] == state,
            "refreshed state was not retained in configuration",
        )
        (root / "refresh.json").write_text(
            json.dumps(
                {"params": parameters, "response": response, "observed": observed},
                indent=2,
            )
            + "\n"
        )
        second = start_turn(backend, thread)
        require(
            backend.settle_turn(thread, second)["status"] == "completed",
            "refreshed turn failed",
        )
        require(len(requests) == 2, "refresh changed model request count")
        events = [item["input"]["hook_event_name"] for item in owner.requests]
        require(events == ordinary.EVENTS * 2, f"refresh lost callbacks: {events}")
        check_identities(owner, thread, {first, second})
        result = {
            "same_thread": thread,
            "model_requests": 2,
            "callbacks": 4,
            "passed": True,
        }
    stderr = (root / "stderr.log").read_text()
    for warning in [
        "failed to rebuild user config for runtime refresh",
        "failed to reload thread configuration",
    ]:
        require(warning not in stderr, f"runtime refresh was skipped: {warning}")
    (root / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--case", choices=["submit-cancel", "stop-cancel", "refresh"])
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    inputs = ordinary.input_manifest(binary)
    (args.output / "inputs.json").write_text(json.dumps(inputs, indent=2) + "\n")
    results = []
    for name in (
        [args.case] if args.case else ["submit-cancel", "stop-cancel", "refresh"]
    ):
        root = args.output / name
        result = (
            refresh(binary, root)
            if name == "refresh"
            else cancellation(
                binary, root, "UserPromptSubmit" if name == "submit-cancel" else "Stop"
            )
        )
        ordinary.verify_inputs(binary, inputs)
        results.append({"case": name, **result})
        print(json.dumps(results[-1]), flush=True)
    (args.output / "qualification.json").write_text(
        json.dumps({"inputs": inputs, "cases": results}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
