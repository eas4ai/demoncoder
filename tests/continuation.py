#!/usr/bin/env python3
"""Prove second-turn context and file continuity, including after cancellation."""
import argparse
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
import uuid

sys.dont_write_bytecode = True
from terminal_session import BINARY, FIXTURE, until
from steering import Provider as ToolProvider
from continuation_fixture import SECOND_PROMPT, new_state, initial_calls, next_call


def retained_results(history, openai):
    if openai:
        calls = [item["call_id"] for item in history if item.get("type") == "function_call"]
        results = [json.loads(item["output"]) for item in history if item.get("type") == "function_call_output"]
    else:
        blocks = [block for item in history if isinstance(item["content"], list) for block in item["content"]]
        calls = [block["id"] for block in blocks if block["type"] == "tool_use"]
        results = [json.loads(block["content"]) for block in blocks if block["type"] == "tool_result"]
        for index, item in enumerate(history):
            if item["role"] == "assistant" and isinstance(item["content"], list):
                uses = {b["id"] for b in item["content"] if b["type"] == "tool_use"}
                if uses:
                    following = history[index + 1]["content"]
                    assert isinstance(following, list), "Anthropic tool uses lack their immediate result message"
                    assert uses == {b["tool_use_id"] for b in following if b["type"] == "tool_result"}, "Anthropic tool results are incomplete"
    assert sorted(calls) == sorted(r["call_id"] for r in results), "conversation has unanswered or duplicate tool calls"
    return {r["call_id"]: r for r in results}


class Provider(ToolProvider):
    def complete_response(self, text):
        if self.path == "/responses":
            self.emit({"type": "response.output_text.delta", "delta": text})
            self.emit({"type": "response.completed", "response": {"output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}]}})
        else:
            self.emit({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}})
            self.emit({"type": "message_stop"})

    def do_POST(self):
        try:
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            history = body["input"] if self.path == "/responses" else body["messages"]
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            state = self.server.state
            if self.server.forget and history[-1].get("content") == SECOND_PROMPT:
                history = history[-1:]
            if not state["turn"]:
                state["turn"] = 1
                self.calls(initial_calls(state))
                return
            results = retained_results(history, self.path == "/responses")
            if history[-1].get("content") == SECOND_PROMPT:
                assert results["create"]["success"], "completed write result was lost"
                assert state["secret"] in json.dumps(history), "unique function was lost from context"
                if state["cancel"]:
                    assert not results["held"]["success"] and not results["unstarted"]["success"]
                    assert results["held"]["exit_code"] is None
                else:
                    assert "MEMORY-" + state["secret"] in json.dumps(history), "assistant context was lost"
                state["turn"] = 2
                self.server.audit = results["create"]
                self.calls([next_call(state)])
            elif state["turn"] == 1:
                assert results["create"]["success"] and not state["cancel"]
                self.complete_response("MEMORY-" + state["secret"])
            else:
                last = list(results.values())[-1]
                call = next_call(state, last)
                self.calls([call]) if call else self.complete_response("CONTINUED-" + state["secret"])
        except (AssertionError, KeyError, IndexError) as error:
            self.server.errors.append(str(error))
            self.emit({"type": "error"})


def case(adapter, server):
    with tempfile.TemporaryDirectory(prefix="demoncoder-continue-") as directory:
        workspace = Path(directory)
        subprocess.run(["git", "init", "-q", directory], check=True)
        (workspace / "continuation").write_text("cancel" if server.state["cancel"] else "complete")
        if server.forget:
            (workspace / "forget-context").touch()
        settings = f'default_connection = "selected"\n[connections.selected]\nadapter = "{adapter}"\nmodel = "fixture-model"\n'
        native = adapter in ("openai-api", "anthropic-api")
        if native:
            route = "responses" if adapter == "openai-api" else "messages"
            settings += f'endpoint = "http://127.0.0.1:{server.server_port}/{route}"\n'
        else:
            settings += f'binary = {json.dumps(str(FIXTURE))}\n'
        config = workspace / "connection.toml"
        config.write_text(settings)
        log = workspace / "events.jsonl"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 160, 0, 0))
        env = {"PATH": "/usr/bin:/bin", "HOME": directory, "TERM": "xterm-256color", "LANG": "C.UTF-8", "OPENAI_API_KEY": "fixture-key", "ANTHROPIC_API_KEY": "fixture-key"}
        process = subprocess.Popen([str(BINARY), "--workspace", directory, "--config", str(config), "--event-log", str(log)], stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        try:
            until(master, process, output, b"Prompt")
            os.write(master, ("create-" + uuid.uuid4().hex[:12]).encode() + b"\r")
            until(master, process, output, b"CONTINUE-WAIT" if server.state["cancel"] else b"MEMORY-", timeout=4)
            source = (workspace / "generated.py").read_text()
            secret = source.split("function_", 1)[1].split("()", 1)[0]
            if server.state["cancel"]:
                os.write(master, b"\x1b")
                until(master, process, output, b"cancelled", timeout=2)
            else:
                until(master, process, output, b"complete", timeout=2)
            os.write(master, SECOND_PROMPT.encode() + b"\r")
            until(master, process, output, ("CONTINUED-" + secret).encode(), timeout=5)
            assert not server.errors, server.errors
            assert (workspace / "generated.py").read_text() == source.replace("return 31", "return 38")
            assert not (workspace / "unstarted.txt").exists()
            records = [json.loads(line)["event"] for line in log.read_text().splitlines()]
            results = {event["result"]["call_id"].removeprefix("claude-mcp-"): event["result"] for event in records if event["type"] == "tool_finished"}
            assert results["verify"]["success"] and results["verify"]["exit_code"] == 0
            audit = server.audit if native else json.loads((workspace / "continuation-audit.json").read_text())["create_result"]
            assert audit == results["create"], "completed result changed across turns"
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0
        except AssertionError as error:
            recent = [json.loads(line)["event"] for line in log.read_text().splitlines()][-5:] if log.exists() else []
            raise AssertionError(str(server.errors) if server.errors else f"{error}; recent events: {recent}") from error
        finally:
            if process.poll() is None:
                os.write(master, b"\x11")
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=3)
            os.close(master)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--fault-forget-context", action="store_true")
    args = parser.parse_args()
    failed = []
    for adapter in ["openai-api", "anthropic-api", "codex", "claude"]:
        for cancel in [False, True]:
            server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
            server.state = new_state(cancel)
            server.forget = args.fault_forget_context
            server.errors = []
            server.audit = None
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            scenario = "cancelled" if cancel else "completed"
            try:
                case(adapter, server)
                print("CODE-006", adapter, scenario, "preceding context, file extension, and verification passed", flush=True)
            except (AssertionError, OSError, subprocess.SubprocessError) as error:
                failed.append((adapter, scenario))
                print("CODE-006", adapter, scenario, "FAILED:", str(error), flush=True)
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=2)
    print("cairn: CODE-006: " + ("fail" if failed else "pass"))
    return int(bool(failed))


if __name__ == "__main__":
    raise SystemExit(main())
