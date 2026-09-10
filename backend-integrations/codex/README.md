# Managed Codex compaction integration

This patch adds the PCOMP-003 required compaction relay to the retained Codex
0.153.4 source. Ordinary connections keep the upstream hook implementation.
The patch does not qualify an artifact by itself: qualification includes actual
manual and automatic compaction through the built binary and host adapter.

At CLI or app-server startup, `CODEX_DEMONCODER_COMPACTION_RELAY` carries:

```json
{"protocol":"demoncoder-compaction-v1","source_path":"/absolute/private/hooks.json","source_sha256":"sha256-of-exact-file-bytes","command":"exact relay shell command"}
```

The inherited requirement authorizes those exact declaration bytes. Startup
rejects missing files, mismatched hashes, malformed configuration, and anything
except one synchronous command for each boundary. The declaration is:

```json
{"hooks":{"PreCompact":[{"hooks":[{"type":"command","command":"exact relay shell command","timeout":65}]}],"PostCompact":[{"hooks":[{"type":"command","command":"exact relay shell command","timeout":65}]}]}}
```

Unknown fields, matchers, environment overrides, asynchronous handlers and extra
events are rejected. Timeout must be 1–120 seconds. The startup declaration is
read only from a regular file. Symlinks, directories and FIFOs are rejected;
the Unix open uses nonblocking/no-follow flags and validates the opened descriptor
so replacement between inspection and open cannot turn startup into a FIFO wait.
The startup declaration is
immutable: deleting or changing its file, disabling ordinary hooks, or refreshing
session configuration cannot remove the requirement. Managed sessions suppress
ordinary hooks, plugin hooks and legacy notify. Authentication continues to use
the existing Codex home; no subscription credentials need copying.

Each upstream compact input gains `demonCoderCompaction` with `protocol` and a
fresh UUID `challenge`. The only accepted response shape is:

```json
{"continue":true,"demonCoderCompaction":{"protocol":"demoncoder-compaction-v1","challenge":"copied-current-challenge","hook_event_name":"PreCompact","session_id":"copied-session","turn_id":"copied-turn"}}
```

`continue:false` denies the boundary and may include `stopReason`. Every identity
must match; unknown fields and untyped responses fail. The relay must obtain the
decision over the host's authenticated private channel before emitting this
acknowledgment. The challenge prevents a stale acknowledgment from releasing a
different wait; it is not a replacement for host channel authentication.

The managed process exchange has a total deadline, including stdin, and a 64 KiB
limit on each output stream. Failure, invalid UTF-8, nonzero exit, signal death,
timeout or invalid acknowledgment stops the current operation. PreCompact stops
before compaction. PostCompact holds continuation after the already completed
compaction; it does not undo it. The managed runner uses `/bin/sh -c` on Linux,
ignores refreshable hook shell settings, and cleans up the relay process tree.

`codex --demoncoder-compaction-capability` returns the protocol, source version
and patch version without starting a backend. Qualification also binds the exact
binary digest and build profile in the generated build receipt.

Build from the retained source with `python3 backend-integrations/codex/build.py
--source reference/codex-rust-v0.153.4 --build-root /absolute/empty/build-root`.
The default profile is `dev` with debug information disabled. An independently
built release artifact needs its own receipt and qualification. `qualify.py`
exercises the actual manual/automatic core waits with a live test owner;
`tests/plugin_codex_installed.rs` separately exercises the production adapter and
authenticated host relay. The test relay in `qualify.py` is only a fault injector.

`source-provenance.json` records the source tree and patch digests. `LICENSE` and
`NOTICE` retain the upstream Apache-2.0 notices. The Cargo lock repair changes the
149 workspace package versions from the release archive's `0.0.0` to `0.153.4`;
the patch adds only direct edges to already pinned `codex-hooks`, `sha2` and `libc`.
External dependency package records must remain byte-for-byte equivalent after
TOML decoding. The build script checks that invariant before compiling.
