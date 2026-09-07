#!/usr/bin/env python3
"""A saturated command queue must leave production terminal input responsive."""
import fcntl
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
import time

sys.dont_write_bytecode = True
from terminal_screen import screen_text

ROOT = Path(__file__).resolve().parents[1]


def main():
    build = subprocess.check_output([
        "cargo", "test", "--locked", "--test", "queue_driver", "--no-run", "--message-format=json"
    ], cwd=ROOT, text=True)
    rows = [json.loads(line) for line in build.splitlines()]
    executable = next(row["executable"] for row in rows if row.get("executable") and row["target"]["name"] == "queue_driver")
    for action in ("prompt", "cancel"):
        with tempfile.TemporaryDirectory(prefix="demoncoder-queue-") as directory:
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 160, 0, 0))
            process = subprocess.Popen([executable, "--ignored", "--nocapture"], stdin=slave, stdout=slave, stderr=slave,
                cwd=directory, env={"PATH": "/usr/bin:/bin", "HOME": directory, "TERM": "xterm-256color", "LANG": "C.UTF-8"}, start_new_session=True)
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
                wait(lambda screen: "Working" in screen and "Correction" in screen, "terminal did not start")
                if action == "prompt":
                    os.write(master, b"retained-draft\r")
                    wait(lambda screen: "queue is full" in screen.lower() and "retained-draft" in screen,
                         "full queue did not visibly reject and retain draft")
                    os.write(master, b"-editable")
                    wait(lambda screen: "retained-draft-editable" in screen, "rejected draft is not editable")
                else:
                    os.write(master, b"\x03")
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
    print("REL-002 full command queue: rejected editable draft, cancel input, and quit passed")


if __name__ == "__main__":
    main()
