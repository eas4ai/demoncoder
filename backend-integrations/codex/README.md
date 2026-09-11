# Managed Codex backend integration

The first patch adds the PCOMP-003 required compaction relay to the retained
Codex 0.153.4 source. A second patch adds the private Submit and Stop boundary
described below. Connections without either private requirement keep the
upstream hook implementation.
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

## Model-hook instruction isolation

The same artifact exposes `--demoncoder-model-hook-capability` with protocol
`demoncoder-model-hook-v1` and source version `0.153.4`. The host selects this
mode only for snapshot-bound model hooks, through the inherited
`CODEX_DEMONCODER_MODEL_HOOK=v1` environment value. Ordinary adapter launches
clear that value. Package configuration and model output cannot select it.

This mode skips global AGENTS.md loading, project instruction discovery,
configured base/developer instruction text and instruction files, custom
compaction prompt files, and the legacy executable notification callback.
It applies again when configuration is rebuilt. Authentication storage,
subscription account restrictions and managed access restrictions retain their
original configuration and directory. No credentials are relocated or copied.
The host also disables ambient tools/plugins/services, supplies the hook's
instructions, and uses an owned empty transport working directory. Snapshot
inspection tools return only host-captured evidence.

`tests/plugin_model_codex_isolation.py` exercises the actual binary against a
local TLS model and synthetic ChatGPT login. It compares ordinary and hook
requests, inspection calls and notification effects. This is controlled
transport evidence, not a live subscription-provider check. Existing compaction
qualification must be rerun whenever the artifact changes.


## Private Submit and Stop boundary

The builder applies `managed-compaction.patch` first, then
`managed-ordinary.patch`. `ordinary-provenance.json` binds the exact intermediate
tree, both patch identities, changed file hashes and final prepared tree.
External dependency records remain unchanged. The build receipt identifies the
resulting binary; it does not establish backend or host qualification.

At startup, `CODEX_DEMONCODER_ORDINARY_RELAY` carries:

```json
{"protocol":"demoncoder-ordinary-v1","source_path":"/absolute/private/hooks.json","source_sha256":"sha256-of-exact-file-bytes","submit_command":"exact Submit command","stop_command":"exact Stop command"}
```

The declaration must contain exactly one synchronous command for each of
`UserPromptSubmit` and `Stop`, with a timeout of 1–120 seconds. Startup validates
the regular file and exact bytes and retains an immutable snapshot. Changing
the file or refreshing configuration cannot replace it. Private sessions suppress
ambient hooks and notification commands while retaining authentication and
non-hook managed policy.

Each actual source input retains its session and turn fields and adds
`demonCoderOrdinary`, containing `protocol`, `delivery_id`, `hook_event_name`,
`session_id` and `turn_id`. The response must echo that entire envelope exactly
and include an explicit boolean `continue`. The fresh delivery UUID identifies
one transport exchange; it does not create a host operation or source hook-run
identity. Supported event output is interpreted only after acknowledgment
validation. Submit may add context, and an acknowledged Stop response may request
a correction. Failure, timeout, nonzero exit or invalid acknowledgment holds the
operation and cannot authorize a correction.

The existing managed transport accepts at most 1 MiB input and 64 KiB per output
stream. The ordinary decoder enforces the same output limit. Host integration
must check the serialized response fits; it must not truncate accumulated context.
`--demoncoder-ordinary-capability` reports protocol `demoncoder-ordinary-v1`,
source version `0.153.4` and patch version `1`.

`qualify_ordinary.py --binary <binary> --output <new-directory>` exercises startup
rejection, actual callback acknowledgments and faults, a Stop correction,
snapshot retention, ambient execution controls and output limits against a local
TLS model. `qualify_ordinary_lifetime.py` adds actual callback interruption,
late-response isolation and same-thread configuration refresh checks. These
source-level checks do not establish production host ownership,
cancellation, recovery or full package compatibility. See
[the boundary review](../../docs/reviews/plugin-codex-ordinary-boundary.md) for
current qualification status and limitations.
