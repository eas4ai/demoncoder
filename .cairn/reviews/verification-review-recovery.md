# Verification, review and recovery review

commitment: verification-review-recovery
commit: 30f892db2ee239a51cf8816860fcc76450a50e4e
findings:
  - none: final review and installed-release verification complete
Status: complete

## VERIFY-004 mechanism review

The requirement changed only by splitting its exhaustion/allowance sentence.
Reviewed the requirement, falsifier, shared shell driver, production correction
fixtures and state transitions. The driver builds the real binary, requires
state/storage/workspace/host tests to succeed, and emits each requirement pass
only after its production Python cases return success. No unconditional pass
substitutes for the checks. All runtime and shared fixture dependencies are
declared through Cargo, src, tests, scripts and specification inputs.

The violating cases use a failed shell check and a reviewer that repeatedly
returns findings. Two correction rounds rerun both check and review; a third
round and ordinary prompt produce no provider request. Original failures remain
in history. Interrupted reverification with a zero-round limit cannot admit
ordinary work after reconciliation. That state regression was observed failing
before the history-aware guard was added. The corrected check passed. Review
cancellation and a cumulative deadline stop without accepting a late clear
verdict. `python3 tests/verification_workflow.py --requirement VERIFY-004` passed
against the committed implementation during this review. The successful
fail/correct/pass/review/explicit-accept path is exercised by VERIFY-001 in the
same driver. No mechanism mismatch found.

## VERIFY-005 mechanism review

The requirement changed only by splitting the admission/phase-change sentence.
Reviewed its full allocation contract, the shared driver, allocation/runtime
admission code and production allocation fixtures. One-call and one-tool limits
stop the next effect. The host fixture allows a guarded mutation only when a
second model admission is available for the Oracle. Reported usage is compared
with actual worker/Oracle events, while missing cost remains unknown. A restart
with a larger requested limit still retains the original one-call allocation and
deadline. A one-second task stops a heartbeat command; review delay is exercised
by the same driver's VERIFY-004 cases. Hard token and monetary requests fail
before provider execution. These are failing-behavior canaries paired with the
allowed case, not checks of completion prose.

`python3 tests/verification_workflow.py --requirement VERIFY-005` passed during
this review. `cargo test --locked --lib workflow` passed seven tests, including
serialized consumption, rollback and a rollback detected between admission and
its durable checkpoint. That last case withholds execution while retaining the
spent admission and a completed result. The admission and completion paths are
intentionally distinct. All determining runtime/fixture dependencies are declared.
No mechanism mismatch found.

## VERIFY-006 mechanism review: finding recorded before repair

The sentence split did not change policy. The production crash/restart cases
pass on both native adapters, including pre-execution, mid-mutation and saved
result checkpoints, corrupt records, exclusive opens, private record failure and
changed workspaces. However, reviewing runtime::finish_phase found an exception
that the mechanism did not challenge: incomplete model admissions were marked
reconciled automatically after ordinary cancellation. VERIFY-006 requires the
developer to inspect and explicitly reconcile incomplete operations before
continuing affected work. Billing uncertainty alone does not supply that
inspection. Add a same-process cancelled-model refusal case and preserve the
uncertain admission until /reconcile. Do not mark this mechanism reviewed until
that implementation and its failing/corrected demonstration are complete.

### VERIFY-006 repair demonstration

The new same-process model-cancellation case failed on the committed candidate:
`cancelled model admission was automatically reconciled`. The repair retains
every incomplete native admission as uncertain until an explicit /reconcile.
Both native adapters now refuse a new provider request before inspection and
continue afterward without resetting spent calls or unknown billing.

Ordinary sessions have no acceptance snapshot. They accept a developer inspection
explanation without traversing home files, and require a fresh workspace inspection
on each resume. The production home-directory case retains previous decisions
while proving that a synthetic private file is not copied into the session record.
Explicit tasks still compare their full bounded snapshot before resuming or
accepting. Updated cancellation tests inspect and reconcile interrupted native
requests; all four adapters still stop owned work within two seconds and continue
with retained context. VERIFY-004 and VERIFY-006 production cases and Clippy
passed after this repair. The unused Tokio standard-input feature was removed;
the host supervisor already uses cancellable nonblocking descriptor reads.

### VERIFY-006 mechanism re-review

After the recorded repair, the complete production recovery suite passed against
committed inputs on both native adapters. It covers saved acceptance, native
conversation and original tool results, retained archive allocations, scoped
inspection decisions, unchanged budgets, refusals before inspection, unsafe
records and workspace/connection authority changes. Interrupted operations do
not replay, whether interruption comes from process death or in-process native
model cancellation. Ordinary home-directory recovery does not capture private
files. Codex and Claude backend task/recovery requests are explicitly refused.

The store's 15 tests passed, including corrupt/oversized/nested records, exclusive
writer locks, symlink/hardlink substitution and injected directory-sync failure.
Five focused host lifecycle tests passed, including detached grandchildren,
missing handshake and deep fork chains. Earlier independent review confirmed
that pidfd-based fixture cleanup cannot signal a recycled PID. Private records
retain sensitive conversation/source, so owner-only permissions and finite
retention are material limits, not encryption. The mechanism declares every
repository file used by these checks. No remaining mismatch found.

## Final integration review: finding recorded before repair

Independent production reproduction ran a task check that created a 9,000,000-byte
file after an input file was added following the initial task snapshot. The shell
check succeeded, but post-check capture exceeded the 8 MiB limit before the task
saved its receipt. The durable operation said phase worker and lacked the actual
pre-check snapshot; task.checks remained empty. Persist check admission with task,
generation, command and snapshot before execution. Retain completion and capture
limitations even when post-check capture fails. Add a production regression.

Other examined acceptance, correction, allocation, checkpoint and reviewer paths
had no new concrete finding. Full Rust checks passed 127 tests with six ignored;
Clippy and release build passed. Retained live-provider checks report stale
evidence; no fresh paid provider execution was requested or claimed.

### Verification capture repair demonstration

The new production regression first failed with a verification operation labeled
worker. The repair scopes the existing event sink to verification and persists
task, generation and the actual pre-check snapshot in the same durable admission
as the command. Post-check capture failure retains the exact ToolResult and a
failed receipt with an explicit limitation; it cannot establish acceptance.

The original capture-failure case now passes. A second case kills the app during
verification, then proves attribution survives and the command does not replay.
Independent specification review ran both cases on both native adapters: all
four passed. The complete VERIFY-002 and VERIFY-006 drivers passed, as did seven
workflow unit tests, formatting and Clippy. The optional attribution field keeps
older records readable. A crash between durable completion and receipt publication
retains the original attributed operation for inspection, without synthesizing
a passing receipt.

Independent quality review approved this repair after checking state safety,
record compatibility, phase observers and error boundaries. It was source review;
it did not claim another test run.

## Production self-audit

1. Reviewed the agreed six requirements, session ownership, native adapters and durable state paths before implementation.
2. Kept the workflow in the existing session and executor; the subprocess supervisor addresses a demonstrated owner-death defect.
3. State, capture, storage, allocation and review have separate responsibilities; shared native interruption handling avoids duplicate cleanup logic.
4. External backends refuse unsupported task and restoration guarantees; retained schema and capability checks reject unsafe records.
5. Durable write failures stop admissions; output and capture limitations stay visible. Synthetic credential canaries exercise the existing access boundary.
6. Private records use checked owner-only files and directories, exclusive locks, safe descriptor-relative access, bounded reads and checksum and schema checks.
7. Admission precedes execution; completion precedes reporting. Uncertain work needs explicit inspection and never replays automatically.
8. Finite cumulative allowances and deadlines span phases and restart. Owned descendants stop after cancellation or runtime death.
9. The implementation plan and active todo tracked the work through verified repair, evidence collection and installed-release checks.
10. Actual Rust, PTY, controlled-adapter and installed-release results are listed below; historical live records remain stale.
11. Review defects were recorded before separate fixes and verified with failing/corrected demonstrations.
12. The developer selected the sequence and acceptance policy; routine implementation stayed inside that contract.
13. The final candidate satisfies these rules within the stated limits; no known in-scope defect or unfinished release check remains.
14. Reviewed commands, errors, README and records for direct descriptions of behavior and limitations.

Static analysis limits: Ripwire reports pre-existing/event-size/churn and protocol similarity flags; its name-based test obligations do not account for all dynamic adapter paths. These are not claimed clean. Runtime tests and focused component reviews cover the applicable boundaries.

## Final evidence and installed release

Candidate implementation: 412aca53dfbbde13695e75aceb858f4b97f25a13.
Committed current receipts: 30f892db2ee239a51cf8816860fcc76450a50e4e.
Cairn recorded all six requirements passing on 2026-09-07 at 15:39:53 UTC.
The final Rust run passed 127 tests with six explicit-driver entry points ignored
across 17 suites. Formatting, Clippy with warnings denied, release build and
whitespace checks passed. The six-requirement mechanism exercised production
terminal behavior plus workspace, state, store and host lifecycle tests.

Installed with cargo install --path . --locked. PATH resolves to
/home/shawn/.cargo/bin/demoncoder. Its SHA-256 matches target/release/demoncoder:
86a872155c0641a6084c445744d2725f6a7302196ccc53c3f06a39e1862c7515.
All six production verification drivers passed with their binary redirected to
that installed executable, using disposable workspaces and controlled peers.
This includes acceptance, original results and capture failure, tool-free review,
correction and cumulative allocations, and interrupted recovery on both native
adapters. Installation and these checks complete the plan release sequence.

Earlier documented startup, usability, presentation, sweep, ordinary connection,
access, usage, queue and cancellation checks passed during implementation.
The retained live connection and Oracle checks were rerun after commit and report
stale evidence. No current live-provider success or fresh paid execution is
claimed; this commitment proves its workflow against controlled local peers.

Material limits remain explicit in README: Linux-only host lifecycle support;
bounded, repeated workspace scans rather than atomic filesystem snapshots;
finite private records with owner permissions rather than encryption; cumulative
call and deadline limits rather than unsupported hard token or monetary caps;
and native-only explicit task recovery. Private records retain source and
conversation. Keep them private and avoid concurrent workspace edits during
verification. These are declared capabilities and constraints, not hidden passes.
