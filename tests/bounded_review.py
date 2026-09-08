#!/usr/bin/env python3
"""Complete changed source and explicit context in actual reviewer requests."""
import json
from pathlib import Path
import tempfile
import sys

sys.dont_write_bytecode = True
from audit_remediation import App, provider
import assignable_subagents as children


def bounded_context(server, selected):
    with tempfile.TemporaryDirectory(prefix="demoncoder-bounded-review-") as directory:
        scope = ["--review-context", "support.rs"] if selected else ["--review-changes-only"]
        app = App(directory, server, [*scope, "--check", "test -s change.rs", "--reviewer", "worker"])
        try:
            (app.workspace / "change.rs").write_text("SOURCE_BEFORE")
            (app.workspace / "support.rs").write_text("SELECTED_SUPPORT_CANARY")
            (app.workspace / "large.rs").write_text("LARGE_SUPPORT_CANARY" + "x" * 1_200_000)
            (app.workspace / "other.rs").write_text("OMITTED_SMALL_CANARY")
            app.send("/task review the small source change")
            (app.workspace / "change.rs").write_text("SOURCE_AFTER")
            app.send("/verify")
            assert app.state()["verification"] == "passed", app.events()
            count = len(server.reviews)
            app.send("/review")
            assert len(server.reviews) == count + 1, app.events()
            request = json.dumps(server.reviews[-1])
            for content in ("SOURCE_BEFORE", "SOURCE_AFTER"):
                assert content in request, "changed source was omitted"
            assert ("SELECTED_SUPPORT_CANARY" in request) == selected
            assert "LARGE_SUPPORT_CANARY" not in request
            assert "OMITTED_SMALL_CANARY" not in request
            for identity in ("large.rs", "other.rs", "sha256"):
                assert identity in request, "omitted-source identity missing"
            assert "not reviewed" in request.lower()
            app.send("/accept")
            assert app.state()["accepted"], app.events()
            path, record = app.record()
            assert "review_context" in record["task"]["review"]["evidence"], "configured scope missing from retained review"
            # An unselected file becomes mandatory when changed. It cannot be
            # silently dropped to keep a misleading clear review under the cap.
            app.send("/task review the next source change")
            (app.workspace / "large.rs").write_text("CHANGED_LARGE_SOURCE" + "y" * 1_200_000)
            app.send("/verify")
            count = len(server.reviews)
            app.send("/review")
            assert len(server.reviews) == count, "oversized changed source reached the reviewer"
            assert any("1 MiB" in event["message"] for event in app.events() if event["type"] == "error"), app.events()
            app.send("/accept")
            assert not app.state()["accepted"]
        finally:
            app.close()
        if selected:
            changed = App(directory, server, ["--resume", str(path), "--review-changes-only"], expect_start=False)
            try:
                assert "scope" in changed.output.decode(errors="replace").lower(), changed.output
            finally:
                changed.close()


def delegated_bounded_review():
    server = children.server_fixture()
    try:
        with tempfile.TemporaryDirectory(prefix="demoncoder-bounded-child-") as directory:
            project = children.repository(directory)
            (project / "large.rs").write_text("CHILD_LARGE_CANARY" + "x" * 1_200_000)
            (project / "support.rs").write_text("CHILD_SELECTED_SUPPORT")
            app = children.launch(directory, server, ["--review-context", "support.rs"])
            try:
                calls = [{"name": "write", "arguments": {"path": "greeting", "content": "child result\n"}}]
                record = children.stopped(app, children.delegate(app, server, "anthropic-api", calls=calls))
                root = Path(record["worktree"]["root"])
                assert (root / "large.rs").stat().st_size > 1_200_000, "review scope excluded a verification input"
                app.send("/agent-validate 1")
                app.wait_for(lambda: children.agent(app)["status"] != "validating", timeout=20)
                assert children.agent(app)["status"] == "ready", children.agent(app)
                request = json.dumps(server.reviews[-1])
                assert "CHILD_SELECTED_SUPPORT" in request
                assert "CHILD_LARGE_CANARY" not in request
                assert "large.rs" in request and "not reviewed" in request
                app.send("/agent-integrate 1")
                app.wait_for(lambda: children.agent(app)["status"] != "integrating", timeout=20)
                assert children.agent(app)["status"] == "integrated", children.agent(app)
                assert (project / "greeting").read_text() == "child result\n"
            finally:
                app.close()
    finally:
        server.shutdown()
        server.server_close()


def main():
    server = provider()
    try:
        bounded_context(server, True)
        bounded_context(server, False)
    finally:
        server.shutdown()
        server.server_close()
    delegated_bounded_review()
    print("AUD-004: bounded parent and child reviews preserve complete changed source and omitted identities")


if __name__ == "__main__":
    main()
