#!/usr/bin/env python3
"""Production orchestration through terminal controls and controlled transports."""
import argparse
import json
import tempfile
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from assignable_subagents import ADAPTERS, agent, repository, server_fixture
from verification_workflow import App


def launch(directory, server, extra=()):
    flags = [value for adapter in ADAPTERS for value in ("--agent-connection", adapter)]
    return App(directory, server, [*flags, "--check", "test -s greeting", "--reviewer", "worker",
                                   "--orchestrate", "--judge", "worker", *extra])


def assign(app, server, adapter, *, dependencies=(), calls=None, owned=("greeting",)):
    server.assignment = {"owned_paths": list(owned), "depends_on": list(dependencies),
                         "context": json.dumps({"fixture_calls": calls}) if calls is not None else ""}
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
