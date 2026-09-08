#!/usr/bin/env python3
"""Declared build outputs through production verification and durable recovery."""
import json
from pathlib import Path
import tempfile
import sys

sys.dont_write_bytecode = True
from audit_remediation import App, provider
import assignable_subagents as children


def large_outputs_and_recovery(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-generated-") as directory:
        scope = ["--generated-output", "build"]
        command = "mkdir -p build; printf x >> build/counter; truncate -s 9437184 build/artifact; test -f source.rs"
        app = App(directory, server, [*scope, "--check", command, "--reviewer", "worker"])
        try:
            (app.workspace / "source.rs").write_text("public source")
            (app.workspace / "build").mkdir()
            (app.workspace / "build/note").write_text("GENERATED_CONTENT_CANARY")
            with (app.workspace / "build/artifact").open("wb") as artifact:
                artifact.truncate(9 * 1024 * 1024)
            app.send("/task verify public source with build outputs")
            app.send("/verify")
            assert app.state()["verification"] == "passed", app.events()
            app.send("/verify")
            assert app.state()["verification"] == "passed", app.events()
            assert (app.workspace / "build/counter").read_text() == "xx"
            app.send("/review")
            app.send("/accept")
            assert app.state()["accepted"], app.events()
            path, record = app.record()
            assert '"build"' in json.dumps(record["task"]["baseline"]), "scope was not retained"
            assert "GENERATED_CONTENT_CANARY" not in (path / "state.json").read_text()
            assert "GENERATED_CONTENT_CANARY" not in json.dumps(server.reviews[-1])
            assert "generated" in json.dumps(server.reviews[-1]).lower()
        finally:
            app.close()
        resumed = App(directory, server, ["--resume", str(path), *scope, "--reviewer", "worker"])
        try:
            resumed.send("/task-status")
            _, record = resumed.record()
            assert record["task"]["accepted"], "unchanged scope lost accepted evidence"
        finally:
            resumed.close()
        changed = App(directory, server, ["--resume", str(path), "--generated-output", "other"], expect_start=False)
        try:
            assert "scope" in changed.output.decode(errors="replace").lower(), changed.output
        finally:
            changed.close()


def undeclared_input_changes_fail(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-generated-input-") as directory:
        app = App(directory, server, ["--generated-output", "build", "--check", "printf changed >> source.rs"])
        try:
            (app.workspace / "source.rs").write_text("original source")
            app.send("/task verify inputs remain stable")
            app.send("/verify")
            assert app.state()["verification"] == "failed", app.events()
            _, record = app.record()
            assert "Workspace changed during verification" in record["task"]["checks"][-1]["output"]
            app.send("/accept")
            assert not app.state()["accepted"]
        finally:
            app.close()


def delegated_outputs():
    server = children.server_fixture()
    try:
        with tempfile.TemporaryDirectory(prefix="demoncoder-generated-child-") as directory:
            project = children.repository(directory)
            (project / "build").mkdir()
            (project / "build/note").write_text("parent generated content")
            with (project / "build/artifact").open("wb") as artifact:
                artifact.truncate(9 * 1024 * 1024)
            app = children.launch(directory, server, ["--generated-output", "build"])
            try:
                calls = [
                    {"name": "bash", "arguments": {"command": "test ! -e build && mkdir build && truncate -s 9437184 build/artifact"}},
                    {"name": "write", "arguments": {"path": "greeting", "content": "scoped child source\n"}},
                ]
                record = children.stopped(app, children.delegate(app, server, "anthropic-api", calls))
                results = [event["result"] for event in record["activity"] if event["type"] == "tool_finished"]
                assert len(results) == 2 and all(result["success"] for result in results), results
                root = Path(record["worktree"]["root"])
                assert (root / "build/artifact").stat().st_size == 9 * 1024 * 1024
                app.send("/agent-validate 1")
                app.wait_for(lambda: children.agent(app)["status"] != "validating", timeout=20)
                assert children.agent(app)["status"] == "ready", children.agent(app)
                app.send("/agent-integrate 1")
                app.wait_for(lambda: children.agent(app)["status"] != "integrating", timeout=20)
                assert children.agent(app)["status"] == "integrated", children.agent(app)
                assert (project / "greeting").read_text() == "scoped child source\n"
                assert (project / "build/note").read_text() == "parent generated content"
            finally:
                app.close()
    finally:
        server.shutdown()
        server.server_close()


def main():
    server = provider()
    try:
        large_outputs_and_recovery(server)
        undeclared_input_changes_fail(server)
    finally:
        server.shutdown()
        server.server_close()
    delegated_outputs()
    print("AUD-003: generated outputs preserve source verification, recovery scope and child integration")


if __name__ == "__main__":
    main()
