#!/usr/bin/env python3
"""A real coding session can establish repository facts and report unavailable checks."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
from scrollback import App, Provider


class AssessmentProvider(Provider):
    def do_GET(self):
        if self.path == "/docs":
            body = b"DOCUMENTATION-AVAILABLE"
            self.send_response(200)
        else:
            body = b"CI service unavailable"
            self.send_response(503)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            catalog = {item["name"]: item["description"].lower() for item in body["tools"]}
            assert "outside" in catalog["read"]
            assert "network" in catalog["bash"] and "private credentials" in catalog["bash"]
            results = [json.loads(item["output"]) for item in body["input"] if item.get("type") == "function_call_output"]
            if results:
                expected = self.server.cases[len(results) - 1]
                result = results[-1]
                assert result["success"] == expected[2], (len(results), result)
                expected[3](result["output"])
                self.server.observed = results
            if len(results) == len(self.server.cases):
                text = "ASSESSMENT-COMPLETE: Git and documentation inspected; CI unavailable; old evidence is stale."
                self.event({"type": "response.output_text.delta", "delta": text})
                output = [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}]
            else:
                name, arguments, _, _ = self.server.cases[len(results)]
                output = [{"type": "function_call", "call_id": f"assessment-{len(results)}", "name": name, "arguments": json.dumps(arguments)}]
            self.event({"type": "response.completed", "response": {"output": output, "usage": {"input_tokens": 20, "output_tokens": 5}}})
        except Exception as error:
            self.server.errors.append(str(error))
            self.event({"type": "response.failed"})


def contains(*expected):
    def check(output):
        for value in expected:
            assert value in output, (value, output)
    return check


class UsabilityContract(unittest.TestCase):
    def test_model_receives_git_docs_remote_and_failed_verification_results(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-assessment-") as directory:
            root = Path(directory)
            expected = {}

            def prepare(workspace, home):
                env = {"PATH": "/usr/bin:/bin", "HOME": str(home), "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null"}
                def git(*arguments):
                    return subprocess.check_output(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", *arguments], cwd=workspace, env=env, text=True).strip()
                git("init", "-q", "-b", "main")
                (workspace / "code.txt").write_text("before\n")
                (workspace / "evidence-source.txt").write_text(git("hash-object", "code.txt") + "\n")
                git("add", "code.txt", "evidence-source.txt")
                git("commit", "-qm", "baseline")
                expected["head"] = git("rev-parse", "HEAD")
                origin = root / "origin.git"
                git("init", "--bare", "-q", str(origin))
                git("remote", "add", "origin", str(origin))
                git("push", "-q", "origin", "HEAD:refs/heads/main")
                (workspace / "code.txt").write_text("after\n")
                standards = home / ".codex/BEST_PRACTICES.md"
                standards.parent.mkdir()
                standards.write_text("MACHINE-STANDARD: verify before claiming completion.\n")
                expected["standards"] = standards

            app = App(root, provider=AssessmentProvider, prepare=prepare)
            try:
                app.server.errors = []
                app.server.observed = []
                base = f"http://127.0.0.1:{app.server.server_port}"
                app.server.cases = [
                    ("read", {"path": str(expected["standards"])}, True, contains("MACHINE-STANDARD")),
                    ("read", {"path": ".git/HEAD"}, True, contains("refs/heads/main")),
                    ("bash", {"command": "set -e; git branch --show-current; git status --porcelain; git diff --numstat; git rev-parse HEAD"}, True, contains("main", " M code.txt", "1\t1\tcode.txt", expected["head"])),
                    ("bash", {"command": "git ls-remote origin refs/heads/main"}, True, contains(expected["head"])),
                    ("bash", {"command": f"curl --noproxy '*' --fail --silent --show-error --max-time 2 {base}/docs"}, True, contains("DOCUMENTATION-AVAILABLE")),
                    ("bash", {"command": f"curl --noproxy '*' --fail --silent --show-error --max-time 2 {base}/ci-status"}, False, contains("503")),
                    ("bash", {"command": 'if test "$(git hash-object code.txt)" = "$(cat evidence-source.txt)"; then printf FRESH; else printf STALE; exit 1; fi'}, False, contains("STALE")),
                ]
                self.assertIn("normal reads/network", app.screen())
                app.send(b"Inspect this repository and establish what is verified.\r")
                app.wait(lambda screen: "ASSESSMENT-COMPLETE" in screen, "an assessment with actual verification results")
                self.assertFalse(app.server.errors, app.server.errors)
                self.assertEqual(len(app.server.observed), len(app.server.cases))
                self.assertFalse(app.server.observed[-1]["success"])
                self.assertFalse(app.server.observed[-2]["success"])
                self.assertEqual((app.workspace / "code.txt").read_text(), "after\n")
            finally:
                app.close()


if __name__ == "__main__":
    unittest.main()
