#!/usr/bin/env python3
"""Complete changed source and explicit context in actual reviewer requests."""
import json
import tempfile
import sys

sys.dont_write_bytecode = True
from audit_remediation import App, provider


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
            _, record = app.record()
            assert "review_context" in record["task"]["review"]["evidence"], "configured scope missing from retained review"
            # An unselected file becomes mandatory when changed. It cannot be
            # silently dropped to keep a misleading clear review under the cap.
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


def main():
    server = provider()
    try:
        bounded_context(server, True)
        bounded_context(server, False)
        print("AUD-004: selected context and omitted identities preserve complete changed-source review")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
