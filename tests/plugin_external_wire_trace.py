#!/usr/bin/python3
"""Diagnostic-only bounded byte tee; normal qualification uses direct exec."""

import os
from pathlib import Path
import signal
import subprocess
import sys
import threading


def main():
    directory = Path(sys.argv[1])
    child = subprocess.Popen(
        sys.argv[2:], stdin=subprocess.PIPE, stdout=subprocess.PIPE
    )
    failed = threading.Event()

    def forward_signal(number, _frame):
        if child.poll() is None:
            child.send_signal(number)

    signal.signal(signal.SIGTERM, forward_signal)
    signal.signal(signal.SIGINT, forward_signal)

    def copy(source, destination, name):
        total = 0
        try:
            # Authentication probes and the backend can invoke this wrapper separately.
            with (directory / f"{os.getpid()}-{name}.bin").open("xb") as capture:
                while chunk := os.read(source, 65536):
                    total += len(chunk)
                    if total > 2 * 1024 * 1024:
                        raise RuntimeError("diagnostic wire capture exceeded 2 MiB")
                    capture.write(chunk)
                    capture.flush()
                    remaining = memoryview(chunk)
                    while remaining:
                        written = os.write(destination, remaining)
                        remaining = remaining[written:]
        except Exception as error:
            failed.set()
            print(f"wire capture failed: {error}", file=sys.stderr)
            if child.poll() is None:
                child.kill()
        finally:
            os.close(destination)

    incoming = threading.Thread(
        target=copy, args=(0, os.dup(child.stdin.fileno()), "stdin"), daemon=True
    )
    outgoing = threading.Thread(
        target=copy, args=(child.stdout.fileno(), os.dup(1), "stdout"), daemon=True
    )
    incoming.start()
    child.stdin.close()
    outgoing.start()
    code = child.wait()
    outgoing.join(timeout=5)
    if outgoing.is_alive() or failed.is_set():
        return 125
    return code if code >= 0 else 128 - code


if __name__ == "__main__":
    raise SystemExit(main())
