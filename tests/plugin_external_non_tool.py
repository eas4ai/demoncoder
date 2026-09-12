#!/usr/bin/env python3
"""Drive real source backends through the Rust lifecycle integration fixture.

The Rust test owns production adapter/runtime assertions. This launcher supplies
isolated local model peers and independently checks actual model request counts.
It neither installs source hooks nor manufactures backend callback messages.
"""

import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import subprocess
import threading
import time

from codex_https_fixture import create_server
from installed_backends import fake_codex_auth

PINNED = {
    "claude": "0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0",
    "codex": "c4d77a245a7fcda26f606bb4f726b59ede5fcf0eb322fb7d625a759fc150592a",
}
EXPECTED_REQUESTS = {
    "pass": 1,
    "pass-twice": 2,
    "submit-deny": 0,
    "stop-correct": 2,
    "always-block": 2,
    "cancel-submit": 0,
    "shutdown-submit": 0,
    "cancel-stop": 1,
    "malformed-submit": 0,
    "malformed-stop": 1,
    "timeout-submit": 0,
    "backend-exit-submit": 0,
    "mixed-post-correct": 2,
    "mixed-submit-deny": 1,
    "mixed-cancel-submit": 1,
    "submit-context": 1,
    "submit-context-overflow": 0,
    "submit-context-lifecycle-overflow": 0,
    "empty-stop": 1,
    "mixed-stop-only": 2,
    "mixed-submit-only": 2,
}
TEST = "actual_external_submit_and_stop_use_the_original_owner"
MAX_BODY = 2 * 1024 * 1024
CLAUDE_TEXT_CASES = {
    "plain": (["done"], "done"),
    "multiple": (["alpha", "beta"], "beta"),
    "trimmed": (["alpha", " \nbeta \n"], "beta"),
    "blank-final": (["alpha", "  "], None),
    "empty-middle": (["alpha", "", "beta"], "beta"),
    "zero-width": (["alpha", "\u200bbeta\u200b"], "\u200bbeta\u200b"),
    "bom": (["alpha", "\ufeffbeta\ufeff"], "beta"),
    "next-line": (["alpha", "\u0085beta\u0085"], "\u0085beta\u0085"),
}


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def peer_handler(adapter, requests, errors, lock, text_case, case):
    class Peer(http.server.BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(5)

        def log_message(self, *_args):
            pass

        def do_GET(self):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"models":[],"items":[]}')

        def do_POST(self):
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= MAX_BODY:
                    raise ValueError("model peer input exceeds bound")
                body = json.loads(self.rfile.read(length))
                if adapter == "codex" and not self.path.endswith("/responses"):
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(b"{}")
                    return
                with lock:
                    if len(requests) >= 8:
                        raise ValueError("model peer request count exceeds bound")
                    requests.append(
                        {"time": time.monotonic(), "path": self.path, "body": body}
                    )
                    sequence = len(requests)
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                for row in model_response(adapter, sequence, text_case, case):
                    self.wfile.write(
                        (
                            "event: "
                            + row["type"]
                            + "\ndata: "
                            + json.dumps(row)
                            + "\n\n"
                        ).encode()
                    )
                self.wfile.flush()
            except Exception as error:
                with lock:
                    if len(errors) < 16:
                        errors.append(str(error)[:1024])
                self.close_connection = True

    return Peer


def model_response(adapter, sequence, text_case, case):
    if adapter == "claude":
        rows = [
            {
                "type": "message_start",
                "message": {
                    "id": f"msg_external_{sequence}",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-sonnet-4-6",
                    "content": [],
                    "stop_reason": None,
                    "usage": {"input_tokens": 100, "output_tokens": 0},
                },
            },
        ]
        mixed_tool = case.startswith("mixed-") and sequence == 1
        if mixed_tool:
            rows.extend(
                [
                    {
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": {
                            "type": "tool_use",
                            "id": "external_mixed_write_1",
                            "name": "mcp__demoncoder__write",
                            "input": {},
                        },
                    },
                    {
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": json.dumps(
                                {
                                    "path": "proof.txt",
                                    "content": "external mixed effect\n",
                                }
                            ),
                        },
                    },
                    {"type": "content_block_stop", "index": 0},
                ]
            )
        for index, text in enumerate(
            [] if mixed_tool else CLAUDE_TEXT_CASES[text_case][0]
        ):
            rows.extend(
                [
                    {
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {"type": "text", "text": ""},
                    },
                    {
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "text_delta", "text": text},
                    },
                    {"type": "content_block_stop", "index": index},
                ]
            )
        rows.extend(
            [
                {
                    "type": "message_delta",
                    "delta": {
                        "stop_reason": "tool_use" if mixed_tool else "end_turn",
                        "stop_sequence": None,
                    },
                    "usage": {"output_tokens": 20},
                },
                {"type": "message_stop"},
            ]
        )
        return rows
    item = {
        "type": "message",
        "id": f"msg_external_{sequence}",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "done"}],
    }
    if case.startswith("mixed-") and sequence == 1:
        item = {
            "type": "function_call",
            "id": "fc_external_mixed_write_1",
            "call_id": "external_mixed_write_1",
            "name": "write",
            "arguments": json.dumps(
                {"path": "proof.txt", "content": "external mixed effect\n"}
            ),
        }
    response = {
        "id": f"resp_external_{sequence}",
        "status": "in_progress",
        "output": [],
    }
    if case == "empty-stop":
        return [
            {"type": "response.created", "response": response},
            {
                "type": "response.completed",
                "response": {
                    **response,
                    "status": "completed",
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 5,
                        "total_tokens": 15,
                    },
                },
            },
        ]
    return [
        {"type": "response.created", "response": response},
        {"type": "response.output_item.added", "output_index": 0, "item": item},
        {"type": "response.output_item.done", "output_index": 0, "item": item},
        {
            "type": "response.completed",
            "response": {
                **response,
                "status": "completed",
                "output": [item],
                "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15},
            },
        },
    ]


def source_environment(adapter, root, server):
    home = root / "home"
    home.mkdir()
    environment = {"HOME": str(home)}
    if adapter == "claude":
        environment.update(
            {
                "CLAUDE_CONFIG_DIR": str(home),
                "CLAUDE_CODE_OAUTH_TOKEN": "synthetic-oauth",
                "ANTHROPIC_BASE_URL": f"http://127.0.0.1:{server.server_port}",
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
                "DISABLE_TELEMETRY": "1",
                "DISABLE_ERROR_REPORTING": "1",
                "DISABLE_AUTOUPDATER": "1",
                "HTTP_PROXY": "",
                "HTTPS_PROXY": "",
                "ALL_PROXY": "",
                "NO_PROXY": "127.0.0.1",
            }
        )
    else:
        codex_home = root / "codex-home"
        codex_home.mkdir()
        fake_codex_auth(codex_home)
        (codex_home / "config.toml").write_text(
            'model="gpt-5.4"\ncli_auth_credentials_store="file"\n'
            'forced_login_method="chatgpt"\n[features]\nenable_request_compression=false\n'
        )
        environment.update(
            {
                "CODEX_HOME": str(codex_home),
                "CODEX_CA_CERTIFICATE": str(server.ca_certificate),
                "NO_PROXY": "",
            }
        )
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"]:
            environment[key] = f"http://127.0.0.1:{server.server_port}"
    return environment


def run(args):
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve(strict=True)
    executable = args.test_executable.resolve(strict=True)
    actual_digest = digest(binary)
    if actual_digest != PINNED[args.adapter]:
        raise ValueError("backend executable differs from qualified source")
    workspace = root / "work"
    workspace.mkdir()
    (root / "tmp").mkdir()
    requests, errors, lock = [], [], threading.Lock()
    handler = peer_handler(
        args.adapter, requests, errors, lock, args.claude_text_case, args.case
    )
    server = (
        create_server(root / "tls", handler)
        if args.adapter == "codex"
        else http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    )
    server.daemon_threads = True
    source_env = source_environment(args.adapter, root, server)
    wrapper = root / "backend"
    invocation = [str(binary)]
    if args.trace_wire:
        invocation = [
            "/usr/bin/python3",
            "-B",
            str(Path(__file__).with_name("plugin_external_wire_trace.py").resolve()),
            str(root),
            str(binary),
        ]
    pid_capture = ""
    if args.case == "backend-exit-submit":
        pid_capture = (
            "if '--input-format' in sys.argv or 'app-server' in sys.argv:\n"
            + "    with open("
            + repr(str(root / "backend.pid"))
            + ", 'x') as capture:\n"
            + "        capture.write(str(os.getpid()))\n"
        )
    wrapper.write_text(
        "#!/usr/bin/python3\nimport os, sys\n"
        + "os.environ.update("
        + repr(source_env)
        + ")\n"
        + pid_capture
        + "os.execv("
        + repr(invocation[0])
        + ", ["
        + ", ".join(repr(item) for item in invocation)
        + "] + sys.argv[1:])\n"
    )
    wrapper.chmod(0o700)
    environment = {
        "PATH": "/usr/bin:/bin",
        "HOME": str(root / "home"),
        "TMPDIR": str(root / "tmp"),
        "RUST_BACKTRACE": "1",
    }
    environment.update(
        {
            "DEMONCODER_EXTERNAL_NON_TOOL_" + key: value
            for key, value in {
                "ADAPTER": args.adapter,
                "BINARY": str(wrapper),
                "CASE": args.case,
                "OUTPUT": str(root),
                "WORKSPACE": str(workspace),
            }.items()
        }
    )
    command = [str(executable), "--ignored", "--exact", TEST, "--nocapture"]
    (root / "inputs.json").write_text(
        json.dumps(
            {
                "kind": "production-adapter-controlled-peer",
                "adapter": args.adapter,
                "case": args.case,
                "diagnostic_wire_tee": args.trace_wire,
                "claude_text_case": args.claude_text_case,
                "source_sha256": actual_digest,
                "test_executable_sha256": digest(executable),
                "launcher_sha256": digest(__file__),
                "helper_sha256": {
                    name: digest(Path(__file__).with_name(name))
                    for name in [
                        "codex_https_fixture.py",
                        "installed_backends.py",
                        "provider_metadata.py",
                        "terminal_session.py",
                        "terminal_screen.py",
                        "boundary_fixture.py",
                        "tool_cycle_fixture.py",
                        "result_fixture.py",
                        "plugin_external_wire_trace.py",
                    ]
                },
                "command": command,
                "expected_model_requests": EXPECTED_REQUESTS[args.case],
            },
            indent=2,
        )
        + "\n"
    )
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    process = None
    try:
        with (root / "rust.log").open("wb") as output:
            process = subprocess.Popen(
                command,
                env=environment,
                cwd=workspace,
                stdout=output,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            try:
                code = process.wait(timeout=90)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
                raise RuntimeError(
                    "Rust backend fixture exceeded its enclosing deadline"
                )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        (root / "model-requests.json").write_text(json.dumps(requests, indent=2) + "\n")
        (root / "peer-errors.json").write_text(
            json.dumps(errors + getattr(server, "errors", []), indent=2) + "\n"
        )
    if code != 0:
        raise RuntimeError(
            f"Rust production fixture failed with exit {code}; inspect {root / 'rust.log'}"
        )
    if errors or getattr(server, "errors", []):
        raise RuntimeError("local peer failed; inspect peer-errors.json")
    if len(requests) != EXPECTED_REQUESTS[args.case]:
        raise AssertionError(
            f"actual model requests {len(requests)} != expected {EXPECTED_REQUESTS[args.case]}"
        )
    if args.case in {
        "stop-correct",
        "always-block",
        "mixed-post-correct",
        "mixed-stop-only",
        "mixed-submit-only",
    }:
        marker = (
            "EXTERNAL_POST_CORRECTION"
            if args.case.startswith("mixed-")
            else "EXTERNAL_STOP_CORRECTION"
        )
        if marker in json.dumps(requests[0]["body"]):
            raise AssertionError("Stop feedback appeared before the first response")
        if marker not in json.dumps(requests[1]["body"]):
            raise AssertionError("source continuation omitted the actual Stop feedback")
    if args.case == "submit-context":
        marker = "EXTERNAL_SUBMIT_CONTEXT:"
        expected = marker + "x" * (60 * 1024 - len(marker))
        if expected not in json.dumps(requests[0]["body"].get("input", [])):
            raise AssertionError(
                "source model input omitted or truncated Submit context"
            )
    if (
        not (root / "receipt.json").is_file()
        or not (root / "host-result.json").is_file()
    ):
        raise AssertionError("production fixture did not retain host evidence")
    if args.adapter == "claude" and args.case == "pass":
        record = json.loads((root / "receipt.json").read_text())
        stops = []
        for operation in record["operations"]:
            invocation = operation.get("host_invocation")
            if isinstance(invocation, dict) and "lifecycle" in invocation:
                source = invocation["lifecycle"]["facts"]["source"]["input"]
                if source["hook_event_name"] == "Stop":
                    stops.append(source)
        expected_text = CLAUDE_TEXT_CASES[args.claude_text_case][1]
        if len(stops) != 1 or stops[0].get("last_assistant_message") != expected_text:
            raise AssertionError(
                "retained Stop text differs from actual source text case"
            )
        if expected_text is None and "last_assistant_message" in stops[0]:
            raise AssertionError("blank final source block must omit Stop text")
    (root / "result.json").write_text(
        json.dumps(
            {
                "case": args.case,
                "adapter": args.adapter,
                "model_requests": len(requests),
                "rust_exit": code,
                "claude_text_case": args.claude_text_case,
            },
            indent=2,
        )
        + "\n"
    )
    print(
        json.dumps(
            {
                "adapter": args.adapter,
                "case": args.case,
                "model_requests": len(requests),
                "output": str(root),
            }
        )
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adapter", choices=PINNED, required=True)
    parser.add_argument("--case", choices=EXPECTED_REQUESTS, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--test-executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--claude-text-case", choices=CLAUDE_TEXT_CASES, default="plain"
    )
    parser.add_argument(
        "--trace-wire",
        action="store_true",
        help="Diagnostic only: adds a bounded byte tee and child wrapper",
    )
    args = parser.parse_args()
    if (
        args.case
        in {
            "submit-context",
            "submit-context-overflow",
            "submit-context-lifecycle-overflow",
            "empty-stop",
            "mixed-stop-only",
            "mixed-submit-only",
        }
        and args.adapter != "codex"
    ):
        parser.error(
            "These source context and empty-response cases require --adapter codex"
        )
    if args.claude_text_case != "plain" and (
        args.adapter != "claude" or args.case != "pass"
    ):
        parser.error("non-plain text cases require --adapter claude --case pass")
    if args.case == "backend-exit-submit" and args.trace_wire:
        parser.error(
            "backend-exit-submit requires direct backend execution without --trace-wire"
        )
    run(args)


if __name__ == "__main__":
    main()
