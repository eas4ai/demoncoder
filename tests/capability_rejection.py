#!/usr/bin/env python3
"""A rejected model must fail the turn without selecting another model."""
import http.server
import sys
import threading

sys.dont_write_bytecode = True
from authentication import Provider, run_case


def main():
    peer = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    thread = threading.Thread(target=peer.serve_forever, daemon=True)
    thread.start()
    try:
        for adapter in ("openai-api", "anthropic-api", "codex", "claude"):
            run_case(adapter, "unsupported-model", peer)
            print("CONN-005", adapter, "rejected the requested model without tools, model substitution, or retry")
    finally:
        peer.shutdown()
        peer.server_close()
        thread.join(timeout=2)
    print("cairn: CONN-005: pass")


if __name__ == "__main__":
    main()
