#!/usr/bin/env python3
"""Select an independently registered provider and drive the production terminal."""
import fcntl
import json
import os
from pathlib import Path
import pty
import secrets
import struct
import subprocess
import sys
import tempfile
import termios

sys.dont_write_bytecode = True
from terminal_session import ROOT, until


def main():
    build = subprocess.check_output(["cargo", "test", "--locked", "--test", "registry_driver", "--no-run", "--message-format=json"], cwd=ROOT, text=True)
    artifacts = [json.loads(line) for line in build.splitlines()]
    executable = next(row["executable"] for row in artifacts if row.get("executable") and row["target"]["name"] == "registry_driver" and row["profile"]["test"])
    with tempfile.TemporaryDirectory(prefix="demoncoder-registry-") as directory:
        root = Path(directory)
        config = root / "settings.toml"
        config.write_text('default_connection="custom-profile"\n[connections.custom-profile]\nadapter="independent"\nmodel="independent-model"\n')
        seed = secrets.token_hex(8)
        (root / "seed.txt").write_text(seed)
        log = root / "events.jsonl"
        token = secrets.token_hex(8)
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 180, 0, 0))
        env = {"PATH":"/usr/bin:/bin", "HOME":str(root), "TERM":"xterm-256color", "LANG":"C.UTF-8", "REGISTRY_CONFIG":str(config), "REGISTRY_WORKSPACE":str(root), "REGISTRY_EVENTS":str(log)}
        if "--omit-registration" in sys.argv:
            env["REGISTRY_OMIT_REGISTRATION"] = "1"
        process = subprocess.Popen([executable, "--ignored", "--nocapture"], stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
        os.close(slave)
        output = bytearray()
        try:
            until(master, process, output, b"Prompt")
            os.write(master, token.encode() + b"\r")
            until(master, process, output, f"REGISTERED-{token}-{seed}".encode())
            rows = [json.loads(line) for line in log.read_text().splitlines()]
            assert all(row["connection"] == "custom-profile" for row in rows)
            events = [row["event"] for row in rows]
            assert [event["owner"] for event in events if event["type"] == "ready"] == ["demoncoder"]
            results = [event["result"] for event in events if event["type"] == "tool_finished"]
            assert len(results) == 1 and results[0]["output"] == seed and results[0]["success"]
            os.write(master, b"\x11")
            process.wait(timeout=5)
            assert process.returncode == 0
        except AssertionError as error:
            raise AssertionError(f"{error}; fixture output: {output.decode(errors='replace')[-2000:]}") from error
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            os.close(master)
    print("CONN-002 independent registration, configured selection, native read, and terminal rendering passed")
    print("cairn: CONN-002: pass")


if __name__ == "__main__":
    main()
