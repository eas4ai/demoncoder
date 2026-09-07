# Verify the four findings in the supplied source-review screenshot

Surfaced from: CHAT-002
Captured: 2026-09-07T01:51:07.581Z

The developer supplied a moving-tree source-review screenshot naming host Unix socket visibility in developer_access.rs, bounded-channel backpressure in terminal.rs, Anthropic partial_json accumulation without an argument-size bound, and per-chunk UTF-8 decoding in tools.rs. These are reported source-level findings, not verified reproductions. They predate and remain outside the current chat-presentation change. Recheck current source and use safe isolated reproductions; do not execute a host-service exploit or destructive probe.
