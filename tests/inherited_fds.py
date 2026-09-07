#!/usr/bin/env python3
"""Pass an ambient socket and outside writable file into the production executor."""
import fcntl
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    build = subprocess.check_output([
        "cargo", "test", "--locked", "--test", "inherited_fd_driver", "--no-run", "--message-format=json"
    ], cwd=ROOT, text=True)
    executable = next(row["executable"] for row in map(json.loads, build.splitlines())
                      if row.get("executable") and row["target"]["name"] == "inherited_fd_driver")
    with tempfile.TemporaryDirectory(prefix="demoncoder-ambient-fds-") as directory:
        root = Path(directory)
        workspace = root / "workspace"
        workspace.mkdir()
        outside = root / "outside-canary"
        outside.write_bytes(b"UNCHANGED")
        with outside.open("r+b") as file:
            peer, inherited = socket.socketpair()
            socket_fd = fcntl.fcntl(inherited.fileno(), fcntl.F_DUPFD_CLOEXEC, 127)
            file_fd = fcntl.fcntl(file.fileno(), fcntl.F_DUPFD_CLOEXEC, 257)
            try:
                env = dict(os.environ, PROBE_WORKSPACE=str(workspace),
                           PROBE_SOCKET_FD=str(socket_fd), PROBE_FILE_FD=str(file_fd))
                result = subprocess.run([executable, "confined_executor_closes_ambient_descriptors", "--ignored", "--exact", "--nocapture"],
                                        cwd=ROOT, env=env, pass_fds=(socket_fd, file_fd), capture_output=True, text=True, timeout=15)
                peer.setblocking(False)
                try:
                    contacted = peer.recv(1024)
                except BlockingIOError:
                    contacted = b""
                assert not contacted, f"host socket contacted: {contacted!r}"
                assert outside.read_bytes() == b"UNCHANGED", "outside writable descriptor survived"
                assert result.returncode == 0, result.stdout + result.stderr
            finally:
                os.close(socket_fd)
                os.close(file_fd)
                inherited.close()
                peer.close()
    print("REL-001 production executor closes inherited host socket and outside writable descriptors")


if __name__ == "__main__":
    main()
