# Managed compaction verification

The source-built Linux artifact is recorded in `build-receipt.json`; its source
and patch are recorded in `source-provenance.json`. The artifact uses the dev
profile with debug information disabled. No release-profile or other-platform
qualification is claimed.

Executed checks:

- Original `just test --locked -p codex-hooks`: 176 passed, zero skipped.
- Managed `just test --locked -p codex-hooks`: 179 passed, zero skipped.
  Before implementation, strict acknowledgment tests failed on empty output,
  declaration tests failed on an incorrect source hash, bounded-process tests
  failed on oversized output, and the registry test failed because disabled
  ordinary hooks skipped the required relay. The implemented cases then passed.
- `cargo build --locked -p codex-cli --bin codex`: passed with Rust 1.95.0,
  four jobs and debug information disabled. Capability and version queries passed.
- `build.py --prepare-only`: applied the retained patch to a fresh source copy
  with zero fuzz and verified the entire expected file tree. The external Cargo
  dependency records remained identical. `build.py --reuse` then verified the
  actual build tree, ran all 179 hook tests, built the artifact and wrote its receipt.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s backend-integrations/codex -p 'test_*.py'`:
  four passed. Tampered source, patch, reused candidate and external dependencies
  are rejected; source links cannot escape or hide an untracked directory.
- `qualify.py`: all 36 actual backend cases passed on the rebuilt FIFO-fix
  artifact whose digest matches `build-receipt.json`. Manual and automatic compaction
  each exercised PreCompact and PostCompact with allow, deny, SIGKILL, timeout,
  stale acknowledgment, empty output, plain output, malformed output and nonzero
  exit. The owner and backend remained alive while model request counts were
  inspected. Pre failures made zero compaction requests; post failures preserved
  exactly one completed compaction and prevented model continuation.
- Actual startup: 17 cases passed, covering valid configuration and malformed,
  null, empty, missing, untrusted, mismatched, disabled, asynchronous and duplicate
  requirements, plus FIFO, directory and symlink sources. The old binary hung
  on a FIFO with no writer and exceeded the two-second startup deadline. The
  rebuilt binary rejected it within the deadline, along with directory and
  symlink sources. Source opening checks regular-file metadata, uses nonblocking
  and no-follow flags on Unix, and checks the opened descriptor before reading.
- Ruff format/check on the packaged Python files: passed.
- `just fix -p codex-hooks --locked --allow-no-vcs`: exited successfully without
  edits. Clippy reports four `expect_used` warnings in extraction after exact
  cardinality validation. Those inputs are checked before extraction; malformed
  declarations are covered by the rejection tests.

The archive has no Git metadata, so upstream `just fmt` stops while enumerating
Git files before running its formatters. Targeted Rust 1.95.0 rustfmt completed
successfully on every edited Rust file. The stable formatter reports that the
upstream optional `imports_granularity` setting requires nightly. Bazel is not
installed; this package qualifies the pinned Cargo build, not a Bazel build.
Ripwire quality-delta cannot compare the archive without a Git baseline. Its
explicit-file test gate reports a broad impact inventory, not executed tests;
the full Codex workspace test suite is not claimed here.

`backend-qualification.json` retains the exact backend case results and artifact
digest. The fault-injection relay is a test fixture. Production authenticated
host relay tests live in `tests/plugin_codex_installed.rs` and are owned by the
host integration; backend acknowledgments alone do not prove host authentication.
The broader plugin commitment and installation qualification remain separate.
