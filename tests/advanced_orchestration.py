#!/usr/bin/env python3
"""Production orchestration through terminal controls and controlled transports."""
import argparse
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
        strings = [item["content"] for item in history if isinstance(item.get("content"), str)]
        call, text = None, "Parent available for independent work."
        parsed = role_request(strings[-1]) if strings else None
        if parsed:
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
            elif strings and strings[-1].startswith("assign "):
                request = {"connection": strings[-1].split()[1], "objective": "write child result",
                           "owned_paths": ["greeting"], "context": ""}
                request.update(self.server.assignment)
                call = {"id": f"delegate-{len(self.server.requests)}", "name": "delegate", "arguments": request}
            elif strings and strings[-1].startswith("parent write"):
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


def launch(directory, server, extra=(), *, advisor="openai-api", judge="anthropic-api"):
    flags = [value for adapter in ADAPTERS for value in ("--agent-connection", adapter)]
    return App(directory, server, [*flags, "--check", "test -s greeting", "--reviewer", advisor,
                                   "--orchestrate", "--judge", judge, *extra])


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


def dependencies():
    for adapter in ADAPTERS:
        server = server_fixture()
        try:
            with tempfile.TemporaryDirectory(prefix="demoncoder-orchestration-") as directory:
                project = repository(directory)
                app = launch(directory, server)
                try:
                    first = assign(app, server, adapter)
                    ready(app, first)
                    second = assign(app, server, adapter, dependencies=[first], owned=["dependent"], calls=[
                        {"name": "read", "arguments": {"path": "greeting"}},
                        {"name": "bash", "arguments": {"command": 'test "$(cat greeting)" = "child result" && printf dependent > dependent'}},
                    ])
                    held = agent(app, second)
                    assert held["status"] == "queued" and held["worktree"] is None, held
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--requirement", required=True, choices=[f"ORCH-{i:03}" for i in range(1, 8)])
    requirement = parser.parse_args().requirement
    cases = {"ORCH-001": dependencies, "ORCH-002": dependencies}
    assert requirement in cases, f"Production case for {requirement} is not implemented"
    cases[requirement]()
    print(f"{requirement}: production terminal cases passed", flush=True)


if __name__ == "__main__":
    main()
