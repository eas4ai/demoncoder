#!/usr/bin/env python3
"""A saturated command queue must leave production terminal input responsive."""
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time

sys.dont_write_bytecode = True
from terminal_screen import screen_text
from cancellation import Provider, case as cancellation_case

ROOT = Path(__file__).resolve().parents[1]


def main():
    build = subprocess.check_output([
        "cargo", "test", "--locked", "--test", "queue_driver", "--no-run", "--message-format=json"
    ], cwd=ROOT, text=True)
    rows = [json.loads(line) for line in build.splitlines()]
    executable = next(row["executable"] for row in rows if row.get("executable") and row["target"]["name"] == "queue_driver")
    for action in ("prompt", "cancel", "accepted", "rejected"):
        with tempfile.TemporaryDirectory(prefix="demoncoder-queue-") as directory:
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 160, 0, 0))
            test = "terminal_with_full_command_queue" if action in ("prompt", "cancel") else "terminal_with_delayed_admission"
            process = subprocess.Popen([executable, test, "--ignored", "--exact", "--nocapture"], stdin=slave, stdout=slave, stderr=slave,
                cwd=directory, env={"PATH": "/usr/bin:/bin", "HOME": directory, "TERM": "xterm-256color", "LANG": "C.UTF-8", "QUEUE_ADMISSION": action}, start_new_session=True)
            os.close(slave)
            output = bytearray()

            def collect():
                if select.select([master], [], [], .03)[0]:
                    try:
                        output.extend(os.read(master, 65536))
                    except OSError:
                        pass

            def wait(predicate, description, seconds=2):
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline:
                    collect()
                    if predicate(screen_text(output, 160, 30)):
                        return
                raise AssertionError(f"{action}: {description}\n{screen_text(output, 160, 30)}")

            try:
                wait(lambda screen: ("Correction" if action in ("prompt", "cancel") else "Prompt") in screen, "terminal did not start")
                if action == "prompt":
                    os.write(master, b"retained-draft\r")
                    wait(lambda screen: "queue is full" in screen.lower() and "retained-draft" in screen,
                         "full queue did not visibly reject and retain draft")
                    os.write(master, b"-editable")
                    wait(lambda screen: "retained-draft-editable" in screen, "rejected draft is not editable")
                elif action == "cancel":
                    os.write(master, b"\x03")
                else:
                    root = Path(directory)
                    os.write(master, b"retained-draft\r")
                    wait(lambda screen: (root / "admission-request").exists() and "retained-draft" in screen,
                         "submitted draft disappeared before admission")
                    os.write(master, b"-edited")
                    wait(lambda screen: "retained-draft-edited" in screen, "pending draft is not editable")
                    os.write(master, b"\r")
                    wait(lambda screen: "waiting for prompt admission" in screen.lower(), "repeated Enter queued another pending draft")
                    (root / "release-admission").touch()
                    if action == "rejected":
                        wait(lambda screen: "queue is full" in screen.lower() and "retained-draft-edited" in screen,
                             "runtime rejection lost the edited draft or its reason")
                        assert not (root / "accepted-1").exists(), "rejected prompt was admitted"
                    else:
                        wait(lambda screen: "ADMITTED-REPLY" in screen and "retained-draft-edited" in screen,
                             "acceptance lost edits made while waiting")
                        assert (root / "accepted-1").read_text() == "retained-draft"
                    assert not (root / "accepted-2").exists(), "repeated Enter duplicated a pending submission"
                    os.write(master, b"\r")
                    wait(lambda screen: (root / "accepted-2").exists() and "complete" in screen, "retained draft could not be resubmitted")
                    assert (root / "accepted-2").read_text() == "retained-draft-edited"
                    # The accepted unchanged draft is now chat, not editor text.
                    wait(lambda screen: "retained-draft-edited" not in screen.split("┌Prompt", 1)[-1], "accepted draft remains in editor")
                os.write(master, b"\x11")
                deadline = time.monotonic() + 2
                while process.poll() is None and time.monotonic() < deadline:
                    collect()
                assert process.poll() == 0, f"{action}: quit blocked behind saturated queue"
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=3)
                os.close(master)
    print("REL-002 full command queue, delayed admission, edited/rejected drafts, repeated Enter, cancel input and quit passed")
    for adapter in ("openai-api", "anthropic-api", "codex", "claude"):
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
        server.scenario = "tool"
        server.stop = threading.Event()
        server.disconnected_at = None
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            cancellation_case(adapter, server, False, quit_turn=True)
            print(f"REL-002 {adapter}: quit stopped owned tool processes within two seconds", flush=True)
        finally:
            server.stop.set()
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


if __name__ == "__main__":
    main()
