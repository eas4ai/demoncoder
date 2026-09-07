#!/usr/bin/env python3
"""Production orchestration through terminal controls and controlled transports."""
import argparse
from contextlib import contextmanager
import http.server
import json
import os
import signal
import tempfile
import threading
import time
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from assignable_subagents import ADAPTERS, agent, repository
from verification_workflow import App
from terminal_session import Provider
from tool_cycle_fixture import sse_call
from orchestration_backend_fixture import role_request, role_reply, work_request, work_calls


class OrchestrationProvider(Provider):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests.append(body)
        history = body["input"] if self.path == "/responses" else body["messages"]
        last = history[-1]
        strings = [item["content"] for item in history if isinstance(item.get("content"), str)]
        call, text = None, "Parent available for independent work."
        parsed = role_request(strings[-1]) if strings else None
        if parsed:
            assert len(history) == 1 and last["role"] == "user", "role reused prior conversation"
            assert not body.get("previous_response_id"), "role continued a previous response"
            role, evidence = parsed
            assert not body.get("tools"), "supervision role was given coding tools"
            self.server.role_requests.append({"role": role, "evidence": evidence, "request": body})
            call, text = role_reply(role, evidence)
        else:
            prompts = [(index, item["content"]) for index, item in enumerate(history)
                       if isinstance(item.get("content"), str)
                       and item["content"].startswith(("You are assigned child agent", "You are correcting child agent"))]
            if prompts:
                start, prompt = prompts[-1]
                request, round_number = work_request(prompt)
                assert body["model"] == "child-" + request["connection"], body["model"]
                calls = work_calls(request, round_number)
                completed = sum(item.get("type") == "function_call_output" for item in history[start:])
                completed += sum(part.get("type") == "tool_result" for item in history[start:]
                                 if isinstance(item.get("content"), list) for part in item["content"])
                if completed < len(calls):
                    call = {**calls[completed], "id": f"child-round-{round_number}-{completed}"}
                text = f"ORCHESTRATION-WORK-DONE round {round_number}"
            elif isinstance(last.get("content"), str) and last["content"].startswith("assign "):
                request = {"connection": last["content"].split()[1], "objective": "write child result",
                           "owned_paths": ["greeting"], "context": ""}
                request.update(self.server.assignment)
                call = {"id": f"delegate-{len(self.server.requests)}", "name": "delegate", "arguments": request}
            elif isinstance(last.get("content"), str) and last["content"].startswith("parent write"):
                call = {"id": f"parent-{len(self.server.requests)}", "name": "write",
                        "arguments": {"path": "parent-budget-escape", "content": "unbudgeted effect"}}
        if call:
            call.setdefault("id", f"role-tool-{len(self.server.requests)}")
            events = sse_call(self.path, call)
        elif self.path == "/messages":
            events = [{"type": "message_start", "message": {"usage": {"input_tokens": 11}}},
                      {"type": "content_block_delta", "delta": {"type": "text_delta", "text": text}},
                      {"type": "message_delta", "usage": {"output_tokens": 8}}, {"type": "message_stop"}]
        else:
            events = [{"type": "response.output_text.delta", "delta": text},
                      {"type": "response.completed", "response": {"output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}], "usage": {"input_tokens": 11, "output_tokens": 8}}}]
        data = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        try:
            self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            pass


def server_fixture():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), OrchestrationProvider)
    server.requests, server.role_requests, server.assignment = [], [], {}
    server.extra_config = ""
    for adapter in ADAPTERS:
        server.extra_config += f'\n[connections.{adapter}]\nadapter="{adapter}"\nmodel="child-{adapter}"\n'
        if adapter in ("openai-api", "anthropic-api"):
            route = "responses" if adapter == "openai-api" else "messages"
            server.extra_config += f'endpoint="http://127.0.0.1:{server.server_port}/{route}"\n'
        else:
            server.extra_config += f'binary={json.dumps(str(Path(__file__).with_name("orchestration_backend_fixture.py")))}\n'
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def launch(directory, server, extra=(), *, advisor="openai-api", judge="anthropic-api", expect_start=True):
    flags = [value for adapter in ADAPTERS for value in ("--agent-connection", adapter)]
    return App(directory, server, [*flags, "--check", "printf 'executed-check-stdout\\n'; printf 'executed-check-stderr\\n' >&2; test -s greeting", "--reviewer", advisor,
                                   "--orchestrate", "--judge", judge, *extra], expect_start=expect_start)


def assign(app, server, adapter, *, dependencies=(), calls=None, owned=("greeting",), policy=None):
    context = dict(policy or {})
    if calls is not None:
        context["fixture_calls"] = calls
    server.assignment = {"owned_paths": list(owned), "depends_on": list(dependencies),
                         "context": json.dumps(context) if context else ""}
    before = len(app.record()[1]["agents"])
    app.send("assign " + adapter)
    records = app.record()[1]["agents"]
    assert len(records) == before + 1, (app.events(), records)
    return records[-1]["id"]


def ready(app, identifier):
    app.wait_for(lambda: agent(app, identifier)["status"] == "ready", timeout=25)
    return agent(app, identifier)


def settled(app, identifier):
    app.wait_for(lambda: agent(app, identifier)["status"] in ("ready", "stopped", "failed", "cancelled", "uncertain"), timeout=25)
    return agent(app, identifier)


@contextmanager
def scenario(extra=(), *, advisor="openai-api", judge="anthropic-api"):
    server = server_fixture()
    try:
        with tempfile.TemporaryDirectory(prefix="demoncoder-orchestration-") as directory:
            project = repository(directory)
            app = launch(directory, server, extra, advisor=advisor, judge=judge)
            try:
                yield app, server, project
            finally:
                if app.master is not None:
                    app.close()
    finally:
        server.shutdown()
        server.server_close()


def heartbeat_call(name):
    assert name.replace("-", "").isalnum()
    return {"name": "bash", "arguments": {"command": f"while :; do printf x >> {name}; sleep .02; done"}}


def heartbeat(app, identifier, name):
    app.wait_for(lambda: agent(app, identifier)["worktree"] is not None)
    path = Path(agent(app, identifier)["worktree"]["root"]) / name
    app.wait_for(lambda: path.exists() and path.stat().st_size >= 3)
    return path


def role_receipts(record):
    return record["orchestration"]["receipts"]


def receipt_evidence(receipt):
    evidence = receipt["evidence"]
    return json.loads(evidence) if isinstance(evidence, str) else evidence


def backend_requests(directory):
    path = Path(directory) / "orchestration-peer.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def wait_role_request(app, server, adapter, role):
    if adapter in ("codex", "claude"):
        app.wait_for(lambda: any(item["kind"] == "prompt" and role_request(item["prompt"])
                                and role_request(item["prompt"])[0] == role
                                for item in backend_requests(app.root)))
    else:
        app.wait_for(lambda: any(item["role"] == role for item in server.role_requests))


def peer_alive(peer):
    try:
        fields = Path(f"/proc/{peer['pid']}/stat").read_text().rsplit(")", 1)[1].split()
        return fields[0] != "Z" and fields[19] == peer["start"]
    except FileNotFoundError:
        return False


def wait_peer_shutdown(peers, deadline):
    while any(peer_alive(peer) for peer in peers) and time.monotonic() < deadline:
        time.sleep(.01)
    assert not any(peer_alive(peer) for peer in peers), "cancelled subscription role process survived"


def assert_transport_evidence(app, server, receipt):
    adapter = receipt["connection"]["adapter"]
    if adapter in ("openai-api", "anthropic-api"):
        supplied = [(item["role"], item["evidence"], item["request"]["model"]) for item in server.role_requests]
    else:
        supplied = [(*role_request(item["prompt"]), item["model"]) for item in backend_requests(app.root)
                    if item["kind"] == "prompt" and item["adapter"] == adapter and role_request(item["prompt"])]
    assert receipt["connection"]["model"] == "child-" + adapter, "role used a different configured model"
    expected = (receipt["role"], receipt_evidence(receipt), receipt["connection"]["model"])
    assert expected in supplied, "retained evidence differs from actual role transport input"


def queue_capacity():
    for adapter in ADAPTERS:
        with scenario() as (app, server, _):
            first = assign(app, server, adapter, calls=[heartbeat_call("first-heartbeat")], owned=["first-heartbeat"])
            heartbeat(app, first, "first-heartbeat")
            second = assign(app, server, adapter, calls=[heartbeat_call("second-heartbeat")], owned=["second-heartbeat"])
            sibling = heartbeat(app, second, "second-heartbeat")
            third = assign(app, server, adapter)
            assert agent(app, third)["status"] == "queued" and agent(app, third)["worktree"] is None
            app.send("independent parent work")
            app.send(f"/agent-cancel {first}")
            ready(app, third)
            before = sibling.stat().st_size
            time.sleep(.08)
            assert sibling.stat().st_size > before, "individual cancellation stopped a sibling"
            allocations = [event["active"] for event in app.events() if event["type"] == "agent_allocation"]
            assert allocations and max(allocations) <= 2, allocations
            assert not agent(app, first)["status"] in ("running", "preparing", "validating", "integrating")

    with scenario(["--agent-limit", "1"]) as (app, server, _):
        first = assign(app, server, "anthropic-api", calls=[heartbeat_call("bound-heartbeat")], owned=["bound-heartbeat"])
        heartbeat(app, first, "bound-heartbeat")
        for _ in range(31):
            app.send(f"/delegate-after {first} anthropic-api greeting waiting assignment")
        assert len(app.record()[1]["agents"]) == 32
        app.send(f"/delegate-after {first} anthropic-api greeting excess assignment")
        assert len(app.record()[1]["agents"]) == 32, "assignment bound was bypassed"
        assert sum(record["worktree"] is not None for record in app.record()[1]["agents"]) == 1


def invalid_dependencies():
    with scenario() as (app, server, _):
        first = assign(app, server, "anthropic-api")
        ready(app, first)
        for invalid in ([99], [first, first], [0], [-1], [2], "wrong-type"):
            server.assignment = {"depends_on": invalid, "context": ""}
            before = len(app.record()[1]["agents"])
            app.send("assign anthropic-api")
            assert len(app.record()[1]["agents"]) == before, invalid


def dependencies():
    for adapter in ADAPTERS:
        server = server_fixture()
        try:
            with tempfile.TemporaryDirectory(prefix="demoncoder-orchestration-") as directory:
                project = repository(directory)
                app = launch(directory, server)
                try:
                    first = assign(app, server, adapter, policy={"role_delay": {"advisor": .5}})
                    app.wait_for(lambda: agent(app, first)["completed"])
                    second = assign(app, server, adapter, dependencies=[first], owned=["dependent"], calls=[
                        {"name": "read", "arguments": {"path": "greeting"}},
                        {"name": "bash", "arguments": {"command": 'test "$(cat greeting)" = "child result" && printf dependent > dependent'}},
                    ])
                    held = agent(app, second)
                    assert held["status"] == "queued" and held["worktree"] is None, held
                    ready(app, first)
                    assert agent(app, second)["status"] == "queued", "clear review released an unintegrated prerequisite"
                    assert (project / "greeting").read_text() == "developer dirty edit\n"
                    app.send(f"/agent-integrate {first}")
                    app.wait_for(lambda: agent(app, first)["status"] == "integrated")
                    released = ready(app, second)
                    assert (Path(released["worktree"]["root"]) / "dependent").read_text() == "dependent"
                    assert not (project / "dependent").exists(), "dependency scheduling integrated without developer authority"
                    assert (project / "greeting").read_text() == "child result\n"
                finally:
                    app.close()
        finally:
            server.shutdown()
            server.server_close()


def failed_prerequisites():
    for mode in ("failed", "cancelled"):
        with scenario() as (app, server, _):
            policy = {"advisor": ["blocked"]} if mode == "failed" else {}
            first = assign(app, server, "anthropic-api", policy=policy)
            settled(app, first)
            if mode == "cancelled":
                app.send(f"/agent-cancel {first}")
            second = assign(app, server, "anthropic-api", dependencies=[first])
            third = assign(app, server, "openai-api")
            ready(app, third)
            blocked = agent(app, second)
            assert blocked["status"] == "queued" and blocked["worktree"] is None, blocked
            app.send(f"/agent {second}")
            assert str(first) in json.dumps(blocked["orchestration"])
            assert "block" in json.dumps(blocked).lower() or "prerequisite" in json.dumps(blocked).lower(), blocked


def advisor_evidence():
    for adapter in ADAPTERS:
        with scenario(advisor=adapter) as (app, server, project):
            identifier = assign(app, server, adapter)
            result = ready(app, identifier)
            receipts = role_receipts(result)
            assert [receipt["role"] for receipt in receipts] == ["advisor"], receipts
            receipt = receipts[0]
            evidence = receipt_evidence(receipt)
            assert_transport_evidence(app, server, receipt)
            assert evidence["assignment"] == result["request"]
            assert evidence["correction_round"] == 0
            assert "developer dirty edit" in json.dumps(evidence["source_evidence"])
            assert "child result" in json.dumps(evidence["source_evidence"])
            assert evidence["checks"] == result["checks"]
            assert evidence["checks"] and all(check["success"] for check in evidence["checks"])
            assert all(check["exit_code"] == 0 for check in evidence["checks"])
            assert "executed-check-stdout" in evidence["checks"][0]["output"]
            assert "executed-check-stderr" in evidence["checks"][0]["output"]
            assert receipt["connection"]["adapter"] == adapter and not receipt["connection"]["tools_enabled"]
            assert receipt["snapshot"] == result["review"]["snapshot"]
            assert (project / "greeting").read_text() == "developer dirty edit\n"


def advisor_refusals():
    for adapter in ADAPTERS:
        with scenario(advisor=adapter) as (app, server, project):
            identifier = assign(app, server, adapter, policy={"role_tool": "advisor"})
            result = settled(app, identifier)
            assert result["status"] != "ready", result
            assert not (Path(result["worktree"]["root"]) / "forbidden-role").exists()
            assert not (project / "forbidden-role").exists()
    for policy in ({"invalid_role": "advisor"}, {"advisor": [{"verdict": "clear", "findings": ["unresolved"], "explanation": "inconsistent"}]}):
        with scenario() as (app, server, _):
            result = settled(app, assign(app, server, "anthropic-api", policy=policy))
            assert result["status"] != "ready", result
    with scenario(["--check", "printf failed-check-stdout; printf failed-check-stderr >&2; exit 17"]) as (app, server, project):
        result = settled(app, assign(app, server, "anthropic-api"))
        assert result["status"] != "ready", "advisor prose cleared a failed host check"
        failed = [check for check in result["checks"] if not check["success"]]
        assert failed and failed[0]["exit_code"] == 17
        assert "failed-check-stdout" in failed[0]["output"] and "failed-check-stderr" in failed[0]["output"]
        for receipt in role_receipts(result):
            assert_transport_evidence(app, server, receipt)
            assert failed[0] in receipt_evidence(receipt)["checks"]
        assert (project / "greeting").read_text() == "developer dirty edit\n"


def stale_role_retention():
    for role in ("advisor", "worker_response", "judge"):
        for adapter in ADAPTERS:
            original = {"verdict": "findings", "findings": [f"Original {role} finding from {adapter}"],
                        "explanation": f"Original {role} explanation must survive stale source"}
            policy = {"advisor": ["findings"], "worker_response": ["findings"], "judge": ["findings"],
                      role: [original], "role_delay": {role: 1}}
            with scenario(advisor=adapter, judge=adapter) as (app, server, project):
                identifier = assign(app, server, adapter, policy=policy)
                wait_role_request(app, server, adapter, role)
                root = Path(agent(app, identifier)["worktree"]["root"])
                (root / "greeting").write_text("changed while role inspected original snapshot\n")
                result = settled(app, identifier)
                assert result["status"] != "ready" and result["orchestration"]["correction_rounds"] == 0
                receipts = role_receipts(result)
                roles = ("advisor", "worker_response", "judge")
                assert [receipt["role"] for receipt in receipts] == list(roles[:roles.index(role) + 1])
                for field in ("verdict", "findings", "explanation"):
                    assert receipts[-1][field] == original[field], "runtime staleness replaced original role response"
                assert_transport_evidence(app, server, receipts[-1])
                app.send(f"/agent-integrate {identifier}")
                assert (project / "greeting").read_text() == "developer dirty edit\n"


def disputes():
    for adapter in ADAPTERS:
        with scenario(judge=adapter) as (app, server, project):
            identifier = assign(app, server, adapter, policy={
                "advisor": ["findings"], "worker_response": ["clear"], "judge": ["clear"],
            })
            result = ready(app, identifier)
            receipts = role_receipts(result)
            assert [receipt["role"] for receipt in receipts] == ["advisor", "worker_response", "judge"]
            assert receipts[0]["findings"] and not receipts[-1]["findings"]
            assert result["review"]["clear"] and result["orchestration"]["correction_rounds"] == 0
            assert len({receipt["snapshot"] for receipt in receipts}) == 1
            for receipt in receipts:
                assert_transport_evidence(app, server, receipt)
            for receipt in receipts[1:]:
                evidence = receipt_evidence(receipt)
                assert evidence["assignment"] == result["request"] and evidence["checks"]
                assert evidence["source_evidence"] == receipt_evidence(receipts[0])["source_evidence"]
                assert "developer dirty edit" in json.dumps(evidence["source_evidence"])
                assert "child result" in json.dumps(evidence["source_evidence"])
                assert evidence["checks"] == receipt_evidence(receipts[0])["checks"]
                assert evidence["advisor"] == receipts[0], "dispute lost original advisor receipt"
                assert not receipt["connection"]["tools_enabled"]
            assert receipt_evidence(receipts[-1])["response"] == receipts[1], "judge lost original worker response receipt"
            assert receipts[1]["connection"]["adapter"] == adapter
            assert receipts[-1]["connection"]["adapter"] == adapter
            assert (project / "greeting").read_text() == "developer dirty edit\n", "a role authorized integration"

        with scenario(judge=adapter) as (app, server, _):
            result = ready(app, assign(app, server, adapter, policy={
                "advisor": ["findings", "clear"], "worker_response": ["clear"], "judge": ["findings"],
                "corrections": [[{"name": "write", "arguments": {"path": "greeting", "content": "repaired greeting\n"}}]],
            }))
            assert result["orchestration"]["correction_rounds"] == 1
            assert (Path(result["worktree"]["root"]) / "greeting").read_text() == "repaired greeting\n"
            receipts = role_receipts(result)
            assert [receipt["role"] for receipt in receipts] == ["advisor", "worker_response", "judge", "advisor"]
            assert receipts[0]["snapshot"] != receipts[-1]["snapshot"], "correction reused old evidence"
            assert result["checks"][0]["snapshot"] == receipts[-1]["snapshot"]


def dispute_refusals():
    for role in ("worker_response", "judge"):
        for adapter in ADAPTERS:
            with scenario(judge=adapter) as (app, server, project):
                result = settled(app, assign(app, server, adapter, policy={"advisor": ["findings"], "role_tool": role}))
                assert result["status"] != "ready", (role, adapter, result)
                assert not (Path(result["worktree"]["root"]) / "forbidden-role").exists()
                assert not (project / "forbidden-role").exists()
    for policy in ({"judge": ["blocked"]}, {"invalid_role": "judge"}):
        with scenario() as (app, server, project):
            policy = {"advisor": ["findings"], **policy}
            identifier = assign(app, server, "anthropic-api", policy=policy)
            result = settled(app, identifier)
            assert result["status"] != "ready" and result["orchestration"]["correction_rounds"] == 0
            assert [receipt["role"] for receipt in role_receipts(result)][:2] == ["advisor", "worker_response"]
            app.send(f"/agent-integrate {identifier}")
            assert (project / "greeting").read_text() == "developer dirty edit\n"


def correction_bounds():
    for adapter in ADAPTERS:
        with scenario(judge=adapter) as (app, server, _):
            identifier = assign(app, server, adapter, policy={
                "advisor": ["findings"], "worker_response": ["clear"], "judge": ["findings"],
                "corrections": [[{"name": "write", "arguments": {"path": "greeting", "content": content}}]
                                for content in ("correction one\n", "correction two\n", "forbidden third\n")],
            })
            result = settled(app, identifier)
            assert result["status"] != "ready" and result["orchestration"]["correction_rounds"] == 2
            root = Path(result["worktree"]["root"])
            assert (root / "greeting").read_text() == "correction two\n"
            receipts = role_receipts(result)
            assert [receipt["correction_round"] for receipt in receipts] == [0, 0, 0, 1, 1, 1, 2, 2, 2]
            assert len({receipt["snapshot"] for receipt in receipts}) == 3
            app.send(f"/agent-validate {identifier}")
            result = settled(app, identifier)
            assert result["status"] != "ready" and result["orchestration"]["correction_rounds"] == 2
            assert (root / "greeting").read_text() == "correction two\n", "revalidation reset correction rounds"


def correction_check_freshness():
    for adapter in ADAPTERS:
        with scenario(judge=adapter) as (app, server, _):
            result = settled(app, assign(app, server, adapter, policy={
                "advisor": ["findings", "clear"], "judge": ["findings"],
                "corrections": [[{"name": "write", "arguments": {"path": "greeting", "content": ""}}]],
            }))
            assert result["orchestration"]["correction_rounds"] == 1
            assert result["status"] != "ready", "correction reused a prior passing check"
            assert (Path(result["worktree"]["root"]) / "greeting").read_bytes() == b""
            assert any(not check["success"] for check in result["checks"]), "failing correction has no executed failure"


def shared_limits():
    with scenario(["--task-model-calls", "5"]) as (app, server, _):
        app.send("/delegate anthropic-api greeting write child result")
        result = settled(app, 1)
        assert result["status"] != "ready"
        assert app.record()[1]["allocation"]["model_calls"] == 5
        assert not server.role_requests, "advisor bypassed exhausted native allowance"
    with scenario(["--agent-backend-turns", "1"], advisor="codex", judge="claude") as (app, server, _):
        result = settled(app, assign(app, server, "codex", calls=[]))
        assert result["status"] != "ready" and app.record()[1]["backend_invocations"] == 1
        assert not role_receipts(result), "advisor bypassed exhausted backend invocation allowance"
        launches = [item for item in backend_requests(app.root) if item["kind"] == "launch"]
        assert len(launches) == 1, "unavailable advisor started an extra backend process"
    with scenario(["--task-tool-calls", "3"]) as (app, server, project):
        result = settled(app, assign(app, server, "anthropic-api", calls=[
            {"name": "write", "arguments": {"path": "greeting", "content": "admitted\n"}},
        ], policy={"advisor": ["findings", "blocked"], "judge": ["findings"], "corrections": [[
            {"name": "write", "arguments": {"path": "greeting", "content": "unbudgeted correction\n"}},
        ]]}))
        assert result["status"] != "ready" and app.record()[1]["allocation"]["tool_calls"] == 3
        assert result["orchestration"]["correction_rounds"] == 1
        assert (Path(result["worktree"]["root"]) / "greeting").read_text() == "admitted\n"
        assert (project / "greeting").read_text() == "developer dirty edit\n"
    for adapter in ADAPTERS:
        with scenario(["--task-seconds", "2"], advisor=adapter) as (app, server, project):
            identifier = assign(app, server, adapter, calls=[], policy={"role_delay": {"advisor": 10}})
            result = settled(app, identifier)
            assert result["status"] != "ready", "advisor ignored shared deadline"
            app.send("parent write after shared deadline")
            assert not (project / "parent-budget-escape").exists()


def delayed_policy(role):
    policy = {"role_delay": {role: 10}}
    if role != "advisor":
        policy["advisor"] = ["findings"]
    return policy


def cancellation():
    for adapter in ADAPTERS:
        for role in ("advisor", "worker_response", "judge"):
            for operation in ("parent", "shutdown"):
                with scenario(["--agent-limit", "1"], advisor=adapter, judge=adapter) as (app, server, project):
                    first = assign(app, server, adapter, policy=delayed_policy(role))
                    app.wait_for(lambda: agent(app, first)["orchestration"]["stage"] == role)
                    wait_role_request(app, server, adapter, role)
                    second = assign(app, server, adapter)
                    assert agent(app, second)["status"] == "queued"
                    app.send(f"/agent {first}")
                    app.send("independent parent work")
                    states = [event for event in app.events() if event["type"] == "agent_state"]
                    assert any(str(first) in json.dumps(event) and role in json.dumps(event) for event in states), states
                    peers = [item for item in backend_requests(app.root) if item["kind"] == "launch"]
                    started = time.monotonic()
                    if operation == "shutdown":
                        app.close()
                        app.master = None
                    else:
                        os.write(app.master, b"\x1b")
                        app.wait_for(lambda: agent(app, first)["status"] == "cancelled"
                                     and agent(app, second)["status"] == "cancelled", timeout=2)
                    wait_peer_shutdown(peers, started + 2)
                    assert time.monotonic() - started < 2, (adapter, role, operation)
                    time.sleep(.15)
                    queued = agent(app, second)
                    assert queued["status"] == "cancelled" and queued["worktree"] is None, queued
                    assert (project / "greeting").read_text() == "developer dirty edit\n"

        with scenario(["--agent-limit", "1"], advisor=adapter) as (app, server, _):
            first = assign(app, server, adapter, policy=delayed_policy("advisor"))
            app.wait_for(lambda: agent(app, first)["orchestration"]["stage"] == "advisor")
            second = assign(app, server, adapter)
            started = time.monotonic()
            app.send(f"/agent-cancel {first}")
            assert time.monotonic() - started < 2
            ready(app, second)
            assert agent(app, first)["status"] == "cancelled"

        for operation in ("parent", "shutdown"):
            with scenario(["--agent-limit", "1"]) as (app, server, _):
                first = assign(app, server, adapter, calls=[heartbeat_call("cancel-heartbeat")], owned=["cancel-heartbeat"])
                path = heartbeat(app, first, "cancel-heartbeat")
                second = assign(app, server, adapter)
                started = time.monotonic()
                if operation == "shutdown":
                    app.close()
                    app.master = None
                else:
                    os.write(app.master, b"\x1b")
                    app.wait_for(lambda: agent(app, first)["status"] == "cancelled"
                                 and agent(app, second)["status"] == "cancelled", timeout=2)
                assert time.monotonic() - started < 2
                before = path.read_bytes()
                time.sleep(.15)
                assert path.read_bytes() == before, "cancellation/shutdown left descendant effects running"
                assert agent(app, second)["worktree"] is None, "cancellation/shutdown started queued work"


def cancellation_persistence_failure():
    for adapter in ADAPTERS:
        with scenario() as (app, server, _):
            identifier = assign(app, server, adapter, calls=[heartbeat_call("durability-heartbeat")], owned=["durability-heartbeat"])
            original_effect = heartbeat(app, identifier, "durability-heartbeat")
            directory, _ = app.record()
            original_mode = directory.stat().st_mode & 0o777
            event_cursor = len(app.events())
            directory.chmod(0o500)
            try:
                effect = original_effect
                started = time.monotonic()
                app.send(f"/agent-cancel {identifier}")
                assert time.monotonic() - started < 2
                before = effect.read_bytes()
                time.sleep(.15)
                assert effect.read_bytes() == before, "failed persistence prevented individual cancellation from draining the child"
                errors = [event["message"] for event in app.events()[event_cursor:] if event["type"] == "error"]
                assert any("persist session transition" in message for message in errors), errors
            finally:
                directory.chmod(original_mode)


def recovery():
    for adapter in ADAPTERS:
        for phase in ("advisor", "worker_response", "judge", "correcting"):
            server = server_fixture()
            try:
                with tempfile.TemporaryDirectory(prefix="demoncoder-orchestration-recovery-") as directory:
                    project = repository(directory)
                    flags = ["--agent-limit", "1"]
                    app = launch(directory, server, flags, advisor=adapter, judge=adapter)
                    try:
                        policy = delayed_policy(phase)
                        owned = ["greeting"]
                        if phase == "correcting":
                            policy = {"advisor": ["findings"], "judge": ["findings"],
                                      "corrections": [[heartbeat_call("recovery-heartbeat")]]}
                            owned.append("recovery-heartbeat")
                        first = assign(app, server, adapter, policy=policy, owned=owned)
                        if phase == "correcting":
                            effect = heartbeat(app, first, "recovery-heartbeat")
                        else:
                            effect = None
                            app.wait_for(lambda: agent(app, first)["orchestration"]["stage"] == phase)
                            wait_role_request(app, server, adapter, phase)
                        second = assign(app, server, adapter, dependencies=[first])
                        independent = assign(app, server, adapter)
                        assert agent(app, independent)["worktree"] is None
                        original = agent(app, first)
                        waiting = agent(app, second)
                        path, saved = app.record()
                        os.kill(app.process.pid, signal.SIGKILL)
                        app.process.wait(timeout=2)
                        app.close()
                        app = None
                        time.sleep(.15)
                        stopped_bytes = effect.read_bytes() if effect else None
                        requests = len(server.requests)
                        peer_requests = backend_requests(directory)
                        app = launch(directory, server, [*flags, "--resume", str(path)], advisor=adapter, judge=adapter)
                        app.wait_for(lambda: any(event["type"] == "agent_state" for event in app.events()))
                        restored = agent(app, first)
                        assert restored["status"] == "uncertain", restored
                        assert restored["worktree"] == original["worktree"]
                        assert restored["request"] == original["request"]
                        for field in ("dependencies", "correction_rounds", "receipts"):
                            assert restored["orchestration"][field] == original["orchestration"][field], field
                        assert agent(app, second)["request"] == waiting["request"]
                        assert agent(app, second)["orchestration"]["dependencies"] == [first]
                        assert agent(app, second)["worktree"] is None
                        assert app.record()[1]["allocation"]["tool_calls"] == saved["allocation"]["tool_calls"]
                        assert app.record()[1]["allocation"]["model_calls"] == saved["allocation"]["model_calls"]
                        assert app.record()[1]["backend_invocations"] == saved["backend_invocations"]
                        time.sleep(.15)
                        assert len(server.requests) == requests, "resume replayed model work"
                        assert backend_requests(app.root) == peer_requests, "resume launched or replayed a subscription backend"
                        assert agent(app, independent)["worktree"] is None, "resume admitted an eligible queue entry"
                        if effect:
                            assert effect.read_bytes() == stopped_bytes, "orphaned correction continued effects"
                            assert restored["orchestration"]["correction_rounds"] == 1
                        app.send(f"/agent {first}")
                        assert agent(app, first)["status"] == "uncertain", "inspection authorized replay"
                        app.send(f"/agent-reconcile {first} inspected retained worktree and interrupted role")
                        assert agent(app, first)["status"] == "failed"
                        assert agent(app, second)["worktree"] is None
                        app.send("/reconcile inspected parent files and interrupted descendants")
                        app.send(f"/delegate {adapter} greeting new assignment must not resume retained queue")
                        time.sleep(.15)
                        assert agent(app, independent)["worktree"] is None, "new assignment resumed retained queue without /agents-resume"
                        app.send("/agents-resume")
                        ready(app, independent)
                        assert agent(app, second)["worktree"] is None
                        assert (project / "greeting").read_text() == "developer dirty edit\n"
                    finally:
                        if app is not None:
                            app.close()
            finally:
                server.shutdown()
                server.server_close()


def recovery_authority_and_integration():
    server = server_fixture()
    try:
        with tempfile.TemporaryDirectory(prefix="demoncoder-orchestration-identity-") as directory:
            project = repository(directory)
            app = launch(directory, server)
            try:
                first = assign(app, server, "anthropic-api")
                ready(app, first)
                app.send(f"/agent-integrate {first}")
                app.wait_for(lambda: agent(app, first)["status"] == "integrated")
                integrated = agent(app, first)
                path, _ = app.record()
                app.close()
                app = None
                refused = launch(directory, server, ["--resume", str(path)], judge="openai-api", expect_start=False)
                refused.close()
                assert b"judge" in refused.output.lower() or b"delegation" in refused.output.lower(), refused.output
                refused = launch(directory, server, ["--resume", str(path), "--check", "false"], expect_start=False)
                refused.close()
                assert b"check" in refused.output.lower() or b"orchestration settings" in refused.output.lower(), refused.output
                original_config = server.extra_config
                server.extra_config = original_config.replace('model="child-anthropic-api"', 'model="replacement-authority"')
                refused = launch(directory, server, ["--resume", str(path)], expect_start=False)
                refused.close()
                server.extra_config = original_config
                app = launch(directory, server, ["--resume", str(path)])
                assert agent(app, first)["status"] == "integrated"
                assert agent(app, first)["worktree"] == integrated["worktree"]
                assert role_receipts(agent(app, first)) == role_receipts(integrated)
                app.send("/reconcile inspected integrated parent greeting")
                second = assign(app, server, "anthropic-api", dependencies=[first], owned=["dependent"], calls=[
                    {"name": "bash", "arguments": {"command": 'test "$(cat greeting)" = "child result" && printf retained > dependent'}},
                ])
                result = ready(app, second)
                assert (Path(result["worktree"]["root"]) / "dependent").read_text() == "retained"
                assert (project / "greeting").read_text() == "child result\n"
            finally:
                if app is not None:
                    app.close()
    finally:
        server.shutdown()
        server.server_close()


def recovery_limits():
    for adapter, limit in (("anthropic-api", ["--task-model-calls", "4"]), ("codex", ["--agent-backend-turns", "2"])):
        server = server_fixture()
        try:
            with tempfile.TemporaryDirectory(prefix="demoncoder-orchestration-budget-") as directory:
                repository(directory)
                flags = ["--agent-limit", "1", *limit]
                app = launch(directory, server, flags, advisor=adapter)
                try:
                    first = assign(app, server, adapter, calls=[], policy=delayed_policy("advisor"))
                    app.wait_for(lambda: agent(app, first)["orchestration"]["stage"] == "advisor")
                    wait_role_request(app, server, adapter, "advisor")
                    app.send(f"/delegate {adapter} greeting queued after spent allowance")
                    path, saved = app.record()
                    assert saved["agents"][-1]["status"] == "queued"
                    app.process.kill()
                    app.process.wait(timeout=2)
                    app.close()
                    app = None
                    time.sleep(.15)
                    app = launch(directory, server, [*flags, "--resume", str(path)], advisor=adapter)
                    app.send(f"/agent-reconcile {first} inspected interrupted advisor")
                    app.send("/reconcile inspected parent and interrupted advisor")
                    requests, peers = len(server.requests), backend_requests(directory)
                    app.send("/agents-resume")
                    result = settled(app, 2)
                    assert result["status"] == "failed", "restart reset spent admission allowance"
                    assert len(server.requests) == requests and backend_requests(directory) == peers, {
                        "adapter": adapter, "http_before": requests, "http_after": len(server.requests),
                        "peers_before": peers, "peers_after": backend_requests(directory),
                        "allocation_before": saved["allocation"], "allocation_after": app.record()[1]["allocation"],
                        "backend_before": saved["backend_invocations"], "backend_after": app.record()[1]["backend_invocations"],
                    }
                    assert app.record()[1]["allocation"]["model_calls"] == saved["allocation"]["model_calls"]
                    assert app.record()[1]["backend_invocations"] == saved["backend_invocations"]
                finally:
                    if app is not None:
                        app.close()
        finally:
            server.shutdown()
            server.server_close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--requirement", required=True, choices=[f"ORCH-{i:03}" for i in range(1, 8)])
    requirement = parser.parse_args().requirement
    cases = {
        "ORCH-001": (queue_capacity, invalid_dependencies),
        "ORCH-002": (dependencies, failed_prerequisites),
        "ORCH-003": (advisor_evidence, advisor_refusals, stale_role_retention),
        "ORCH-004": (disputes, dispute_refusals),
        "ORCH-005": (correction_bounds, correction_check_freshness, shared_limits),
        "ORCH-006": (cancellation, cancellation_persistence_failure),
        "ORCH-007": (recovery, recovery_authority_and_integration, recovery_limits),
    }
    for case in cases[requirement]:
        case()
        print(f"{requirement}: {case.__name__} passed", flush=True)
    print(f"{requirement}: production terminal cases passed", flush=True)


if __name__ == "__main__":
    main()
