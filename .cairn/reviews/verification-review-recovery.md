# Verification, review and recovery review

commitment: verification-review-recovery
commit: 1ca2dd5b4f10bde845c6e121f7f18a1c2dc1b0f4
findings:
  - open: VERIFY-002/006 verification snapshot and attribution are lost when post-check capture fails
Status: in progress

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
