#!/usr/bin/env python3
"""Audit regressions through the actual terminal, tools and reviewer requests."""
import argparse
import http.server
import json
from pathlib import Path
import sys
import tempfile
import threading

sys.dont_write_bytecode = True
from verification_workflow import App, WorkflowProvider


def provider():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), WorkflowProvider)
    server.tool_cycles = False
    server.received = []
    server.requests = []
    server.reviews = []
    server.oracles = []
    server.edit_work = False
    server.verdict = {"verdict": "clear", "findings": [], "explanation": "Synthetic source and check agree."}
    server.review_tool = None
    server.worker_tool = None
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def private_review(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-private-review-") as directory:
        app = App(directory, server, ["--check", "test -f source.rs", "--reviewer", "worker"])
        try:
            (app.workspace / ".demoncoder").mkdir()
            (app.workspace / ".demoncoder/token").write_text("PRIVATE_RUNTIME_CANARY")
            (app.workspace / ".env").write_text("PRIVATE_ENV_CANARY")
            (app.workspace / ".config/gh").mkdir(parents=True)
            (app.workspace / ".config/gh/hosts.yml").write_text("PRIVATE_GITHUB_CANARY")
            (app.workspace / ".cargo").mkdir()
            (app.workspace / ".cargo/credentials.toml").write_text("PRIVATE_CARGO_CANARY")
            (app.workspace / "source.rs").write_text("public source for review")
            app.send("/task inspect source.rs")
            app.send("/verify")
            count = len(server.reviews)
            app.send("/review")
            assert len(server.reviews) == count + 1, app.events()
            request = json.dumps(server.reviews[-1])
            path, _ = app.record()
            retained = (path / "state.json").read_text()
            for secret in ("PRIVATE_RUNTIME_CANARY", "PRIVATE_ENV_CANARY", "PRIVATE_GITHUB_CANARY", "PRIVATE_CARGO_CANARY"):
                assert secret not in request, "private source reached the reviewer"
                assert secret not in retained, "private source entered durable task evidence"
            assert "public source for review" in request
            assert "excluded" in request
            assert app.state()["verification"] == "passed", app.state()
            assert app.state()["review"] == "clear", app.state()
            app.send("/accept")
            assert app.state()["accepted"], app.state()
        finally:
            app.close()


def configured_credential_alias(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-private-alias-") as directory:
        app = App(directory, server, ["--check", "true", "--reviewer", "worker"])
        try:
            # A configured path outside the project can resolve inside it.
            # This uses only the fixture's synthetic connection settings.
            inside = app.workspace / "custom-settings.toml"
            app.config.rename(inside)
            app.config.symlink_to(inside)
            count = len(server.received)
            app.send("/task inspect project")
            errors = [event["message"] for event in app.events() if event["type"] == "error"]
            assert any("private session or connection settings" in error for error in errors), errors
            assert len(server.received) == count, "private workspace task reached the provider"
            _, record = app.record()
            assert record["task"] is None, "private settings entered a task baseline"
        finally:
            app.close()


def relocated_credentials(server):
    for variable in ("CODEX_HOME", "CLAUDE_CONFIG_DIR", "AWS_SHARED_CREDENTIALS_FILE"):
        with tempfile.TemporaryDirectory(prefix="demoncoder-private-environment-") as directory:
            secret = Path(directory) / "project/runtime-secrets/auth.json"
            secret.parent.mkdir(parents=True)
            secret.write_text("DYNAMIC_TERMINAL_PRIVATE_CANARY")
            value = secret if variable == "AWS_SHARED_CREDENTIALS_FILE" else secret.parent
            app = App(directory, server, ["--check", "true", "--reviewer", "worker"],
                      environment={variable: str(value)})
            try:
                count = len(server.requests)
                app.send("/task inspect public project")
                errors = [event["message"] for event in app.events() if event["type"] == "error"]
                assert any("private" in error and "workspace" in error for error in errors), errors
                assert len(server.requests) == count, "unsafe task reached a provider"
                path, record = app.record()
                assert record["task"] is None, "unsafe task retained a baseline"
                assert "DYNAMIC_TERMINAL_PRIVATE_CANARY" not in (path / "state.json").read_text()
            finally:
                app.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", choices=["privacy"], default="privacy")
    parser.parse_args()
    server = provider()
    try:
        private_review(server)
        configured_credential_alias(server)
        relocated_credentials(server)
        print("AUD-001: production review excludes private source and preserves acceptance")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
