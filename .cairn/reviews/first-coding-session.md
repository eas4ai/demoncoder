# First coding session implementation review

commitment: first-coding-session
commit: c6522bd6f570758d70fe3d7a7ad4813836f45302
findings:
  - none: no open findings within this commitment
Reviewer: Codex
Date: 2026-09-06
Status: complete
Open findings: none

## Scope and evidence

Reviewed the agreed CODE-001 through CODE-010 and CONN-001 through CONN-006,
the eight implementation decisions, current mechanisms, and their failure
demonstrations. The current coding receipts are dated 20260906T151828626Z
and 20260906T151828627Z. The current connection receipts are dated
20260906T152010595Z and 20260906T152010596Z. All sixteen requirements pass.

Each of the four current live records completes a two-turn task with actual
read, write, edit, and Bash results and independently checked source. These
records are dated 20260906T151901008117Z (OpenAI), 20260906T151901022048Z
(Codex), 20260906T151901038844Z (Claude), and 20260906T151902094013Z
(Anthropic). The current live Oracle allow/deny pair is dated
20260906T151625164157Z. Its proposals were judged and never executed.

Additional checks ran against the current implementation: cargo test
--locked --all-targets passed 14 tests across seven suites with two ignored
entry points; cargo fmt --check, cargo clippy --locked --all-targets -- -D
warnings, and git diff --check passed. The two ignored entry points are
intentionally driven separately: the registry fixture through its real PTY
driver, and the live Oracle through its explicit live driver. Both passed.
The Markdown audit found no missing local targets in 37 tracked documents.
A credential-pattern scan found no suspect tracked files; it is a narrow
pattern check, not proof that arbitrary secrets cannot exist.

## What I challenged

- Lifecycle and cancellation: traced main, session, native progression, and
  backend process ownership. The terminal runs separately from model/tool
  work. Cancellation closes pending native calls without inventing success.
  Completed receipts survive interrupted delivery. Backend cleanup signals
  the process group before reaping its leader. Tests cover partial JSON,
  exited leaders with live helpers, cancellation, and a subsequent prompt.
- Final admission: inspected typed arguments, hook ordering, call identity,
  rooted file descriptors, Bash construction, and host review. Confined
  execution uses openat2 and mandatory bubblewrap. Host execution requires
  explicit selection. The Oracle sees final arguments, cannot obtain tools,
  and must complete with a valid allow decision before an outside effect.
  Existing canary tests cover transformed denials, target replacement,
  hard links, failed review, timeout, and cancellation without destructive
  probes. Installed backend tests challenge built-in and inherited tools.
- Settings and authentication: examined trusted configuration selection,
  environment precedence, private file validation, setup locking, hidden
  input, synchronization, and atomic settings replacement. Selected API and
  subscription routes remain distinct. Fixture checks reject missing,
  expired, and mismatched routes without fallback. Private credentials do
  not enter ordinary tool environments or retained application events.
- Extension and ownership boundaries: registration checks required controls
  before factory creation. A separate adapter runs through the unchanged
  session and terminal. Backend continuation checks the original identity;
  controlled wrong-identity cases fail before a queued write is admitted.
  The declaration is a trusted adapter promise; it cannot make arbitrary
  third-party code safe or prove a backend's future behavior.
- Evidence and presentation: examined EventSink, retained tool results,
  separate presentation events, terminal control-character filtering, and
  usage formatting. Logs receive the actual event before UI delivery.
  The usage test reads the current screen and exact attributed events;
  historical terminal bytes cannot satisfy its assertion. Missing fields
  remain unknown, explicit zeros remain zero, and new turns reset usage.
  The safe unknown-as-zero fault fails on every adapter and the corrected
  implementation passes. Oracle usage retains its own visible identity.

No code was changed during this review. No new defect within the selected
commitment was found. The stale usage footer was corrected and verified as
CONN-006 work before this review began.

## Limits retained

The live tasks establish transport and session/tool operation for the tested
accounts, models, and installed backend versions. They do not establish
arbitrary coding quality or future service compatibility. The default is
Linux-specific. Host mode is explicitly unsandboxed; Oracle decisions can
be wrong, and process-group cleanup does not cover deliberate detachment
into another session. The implementation documentation states these limits.

The footer shows the latest usage report within a turn, not cumulative
billing. Unreported service charges remain unknown. Event files are optional
session evidence, not a crash-recovery journal. Crash recovery, cumulative
allocations, advanced review, subagents, orchestration, and improvement
remain later commitments. This review does not claim those capabilities.

## Production self-audit

The implemented behavior matches the selected contract, recorded decisions,
and executable evidence. The changes preserve the small shared loop and
explicit ownership, validate the relevant boundaries, handle errors without
credential fallback, and document their practical limits. Required checks
ran and passed. There are no known unresolved defects or unfinished items
inside this commitment. No further revision is needed for this deliverable.
