#!/usr/bin/env python3
"""Exercise delegated work through the actual terminal and adapter boundaries."""
import argparse
import http.server
import subprocess
import tempfile
import threading
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from verification_workflow import App, WorkflowProvider


def server_fixture():
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), WorkflowProvider)
    server.tool_cycles = False
    server.received = []
    server.requests = []
    server.reviews = []
    server.edit_work = False
    server.verdict = {"verdict": "clear", "findings": [], "explanation": "Fixture review"}
    server.review_tool = None
    server.worker_tool = None
    server.oracles = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def terminal_assignment(server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-subagent-") as directory:
        project = Path(directory) / "project"
        project.mkdir()
        subprocess.run(["git", "init", "-q", str(project)], check=True)
        subprocess.run(["git", "-C", str(project), "-c", "user.name=Fixture",
                        "-c", "user.email=fixture@example.invalid", "commit",
                        "--allow-empty", "-qm", "fixture baseline"], check=True)
        app = App(directory, server, ["--agent-connection", "worker"])
        try:
            app.send("/delegate worker greeting inspect the assigned worktree")
            app.wait_for(lambda: any(e["type"] == "agent_state" for e in app.events()))
            state = [e for e in app.events() if e["type"] == "agent_state"][-1]
            assert state["connection"] == "worker", state
            assert Path(state["worktree"]) != project, "child shares the parent workspace"
            app.send("/agents")
        finally:
            app.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--requirement", required=True)
    args = parser.parse_args()
    server = server_fixture()
    try:
        if args.requirement == "SUB-001":
            terminal_assignment(server)
        else:
            raise AssertionError(f"production coverage not implemented for {args.requirement}")
    finally:
        server.shutdown()
        server.server_close()
    print(f"{args.requirement}: production terminal cases passed")


if __name__ == "__main__":
    main()
