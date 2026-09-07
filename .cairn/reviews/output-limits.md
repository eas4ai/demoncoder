# Output limit implementation review

commitment: output-limits
commit: 4554457a51761a40e1184b0897d774ec216be811
findings:
  - none: no open findings within this commitment
Reviewer: Codex
Date: 2026-09-07
Status: complete
Open findings: none

## Evidence and failure demonstrations

Cairn recorded OUTPUT-001, OUTPUT-002 and OUTPUT-003 as passing against
committed inputs at 20260907T010432246Z / 20260907T010432247Z.
The mechanism runs 12 Rust adapter/session tests, three terminal tests,
three onboarding tests and the configuration test. Its shell exits on any
failed command before emitting the requirement pass lines.

During development, the regression tests failed against the former fixed
4096 request, accepted truncation, ignored metadata and unsupported output
setting. The corrected cases pass. This was development test evidence,
not a committed Cairn baseline receipt. Controlled endpoints inspect actual
requests and enforce the old cap in the large-file case: a cap below 12000
returns an incomplete tool argument; the discovered 128000 permits a 50000-byte
file and successful tool-result continuation. These are synthetic protocol
checks, not a paid large-generation benchmark.

The aggregate Rust run passed 44 tests with three explicitly ignored tests.
Two subsequently added metadata tests also pass in the final 12-test focused
suite. All-target Clippy with warnings denied, formatting and diff whitespace
checks passed. Terminal tool cycles, authentication, usage, setup, startup,
steering, cancellation, responsiveness, installed-backend boundaries and result
handling, scrollback, continuation and the usability contract also passed.
The installed-backend fixture initially failed because its existing GET handler
shadowed shared metadata handling. It now delegates individual model requests;
its boundary and result checks pass for all four adapters.

## What I challenged

- Discovery uses the selected credential and model, percent-encodes model path
  components, stays on the endpoint origin and caches only a positive u32 result.
  Missing, null, negative, zero, nonnumeric and oversized integer limits fail.
  Explicit native limits bypass discovery; OpenAI omission remains omitted.
- The shared HTTP client disables redirects and omits reflected error bodies.
  An actual redirect to a second controlled server produces no request there.
  Invalid JSON and metadata beyond 64 KiB fail without reflecting their bodies.
  The metadata request has a 30-second deadline; native cancellation drops its
  future, and the cancellation test observes no message admission.
- Anthropic checks both max_tokens and model_context_window_exceeded before
  parsing or returning any tool calls. Even valid-looking calls in a truncated
  response have no effects. Usage is emitted first. Failed partial output is
  not appended to history, and the next prompt has no unfulfilled tool records.
  OpenAI incomplete responses similarly retain usage and never return calls.
- NativeSession admits tools only after the entire adapter response succeeds.
  Discovery and streaming both run inside its command-select cancellation path.
  This change does not bypass hooks, tool validation or workspace access policy.
- CLI limits are positive; saved settings are preserved and invocation overrides
  reach both APIs. Setup persists an explicit limit. External backends reject
  the option instead of silently ignoring it. No credential or model fallback
  was added. Provider rejection of an excessive explicit limit remains an error.
- README flags, settings, precedence, metadata limitations and incomplete-turn
  behavior agree with source. Output-token limits remain distinct from context
  size, byte bounds and later cumulative budgets or context compaction.

No production code changed during this completion review. No live provider
capability or paid-generation run is claimed for this correction. Provider
metadata can be unavailable on compatible endpoints; the documented explicit
limit is the supported path there.

## Production self-audit

The correction is scoped to the selected commitment, uses existing adapter and
configuration boundaries, preserves cancellation and credential handling, and
has meaningful success/failure evidence. Runtime, tests, configuration and manual
agree. No unresolved finding or further revision is needed for this commitment.

## Installed verification

The release installed with cargo install --path . --locked --force. All three
output-limit terminal tests passed against /home/shawn/.cargo/bin/demoncoder.
Installed help exposes the new option and the CLI rejects a zero limit.

The developer requested version 0.1.1. The only declared input changes since
the initial review are the package version in Cargo.toml and the matching
Cargo.lock package entry; no dependencies or runtime code changed. Locked build,
six startup tests, and the installed version check pass. Cairn refreshed all three
OUTPUT receipts at 20260907T010706124Z. The 0.1.1 release was reinstalled and all
three output-limit terminal tests pass against the installed executable.
The packaging review found no new issue.
