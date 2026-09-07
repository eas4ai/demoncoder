# Advanced orchestration review

commitment: advanced-orchestration
commit: c7db3bcd8ebc537e20b97999f6312d10d67ece44
findings:
  - open: Orchestration implementation and production proof are incomplete.
Status: in progress

## Baseline demonstration

The first declared check was unverified because its script did not yet exist;
that receipt proves no runtime behavior. The production dependency test was then
run against the installed-development candidate and failed at actual CLI startup:
`unexpected argument --orchestrate found`. The child workflow cannot start on
the baseline. Other requirement cases explicitly fail until implemented; no
missing test is represented as passing evidence.

## Scheduling component review

Specification review independently passed all twelve tests and approved the
pure graph predicate at dbedbbde217cb4cf4b7c7cbaa53596c99cdb25d3. Quality review
found an omitted falsifier: none of those cases provides more eligible queued
assignments than remaining capacity. Removing the final capacity truncation
would therefore still pass. Add an unsorted overfull eligible queue and a fully
occupied limit, demonstrate the violating implementation fails, then re-review.
This finding is recorded before the test repair.

### Scheduling capacity repair

Removing ready.truncate(capacity) made the new test fail with IDs [10,70,90]
instead of [10]. Restoring it passed all thirteen cases. Both specification
and quality reviewers independently reran thirteen passing tests and approved
5f01da072eaf0d7dfaebb220c328bb2972a8bdad. The reviewed branch was merged with
no fast-forward; there is no remaining component finding.

## Production fixture specification review (in progress)

Before repairs, independent review found these gaps in the draft proof:
role peers accepted only assignment and round, without requiring source, checks,
original advisor findings or worker response; assertions inspected retained
receipts without comparing actual transport input. Recovery counted HTTP requests
but had no subscription peer request log, so silent backend replay could escape.
Recovery also did not challenge changed connection authority or retain an already
integrated prerequisite, and ample limits did not demonstrate spent admission after
restart. These are proof findings, not observed runtime defects. Production
execution remains pending the coupled manager implementation.

Further fixture findings: changed nonempty content cannot distinguish rerun checks
from a relabeled prior success; add a correction that fails a formerly passing
check. Status/owner shutdown alone does not establish subscription role process
termination; observe launched peer process identities and an active worker
heartbeat during shutdown. These findings are recorded before those repairs.

### Draft proof coverage repair

The specification reviewer approved the repaired fixture design. Actual role
transport input is compared with persisted evidence and configured model identity;
dispute inputs must contain the exact original preceding receipts. Stripped source
and summary-only evidence were rejected in an independent peer experiment for all
three roles. Added failed-check correction, subscription process termination,
launch/request replay detection, changed-authority refusal, retained integration
and spent-admission recovery cases. This closes the draft coverage findings;
production execution and final mechanism assessment remain pending.

## Draft proof quality review (in progress)

The reviewer demonstrated that HTTP role peers accepted earlier worker/assistant
conversation before the final supervision prompt. Matching the final evidence
does not establish a fresh role context. Add a transport-level context check and
reject reused worker conversation. This finding is recorded before the repair.

The quality review also found that silent checks cannot expose erased stdout or
stderr, and recovery can kill between a persisted role stage and its actual
transport request. Add distinctive executed output on successful and failed
checks and gate interruption on the selected role request. These are recorded
before repair; runtime defects have not been inferred from the proof gaps.

### Draft proof quality repair

The reviewer independently confirmed fresh role contexts succeed and contaminated
or reused contexts fail across all four peers. Check output now has distinct stdout
and stderr markers, including an exit-17 failure, and recovery waits for the actual
selected role request before interruption. These three findings are closed.

A further precision issue was found before repair: the correction tool allowance
case spent its two tools on parent delegation and the first child write, so it
stopped at initial verification. Use three tool admissions and require correction
round one to prove the corrective write itself is refused. This is a proof change,
not an observed runtime defect.

### Production probe and fixture approval

The fixture quality reviewer approved the final proof component with no open
finding. The correction allowance case now requests one correction, observes its
denied write with three tools spent, then returns a blocked advisor verdict.
Independent parent runs passed all ORCH-005 and ORCH-007 cases against a copied
debug candidate with SHA-256
9006a68d18e1375d68a0b98285efc0a5a56ea7d3e501f909c532b32d07ace05e.
The worker's recovery test had exposed a subscription process launch before
exhausted backend admission; its preflight repair is present in this candidate.
These are editing-time probes, not Cairn receipts or installed-release evidence.
Runtime source review, remaining regressions and final checks are still pending.

## Runtime self-review before handoff

The implementer found and is repairing three issues before independent source
review: selected checks were not frozen in resumed orchestration identity;
individual cancellation could return on a failed durable write before draining;
and role-admission bookkeeping used the global operation count instead of the
assignment's exact phase under concurrent work. The parent is adding a production
changed-check refusal case. The prior seven ORCH, seven SUB and six VERIFY manual
passes apply to copied candidate 9006a68d only, before these further repairs.

While preparing the refusal falsifier, the parent found that the shared Python
App constructor did not close an unexpectedly accepted application when its
expected-startup-refusal wait timed out. Repair the fixture's exception cleanup
before demonstrating the violating case, so the failed test leaves no owner alive.

### Incremental fixture review

Before repair, the reviewer found that the failed-persistence cancellation case
accepted any historical error instead of a new persistence error, and that the
shared App.close timeout killed the process but raised before closing its PTY.
Require the new persistence diagnostic after a captured event cursor and close the
PTY in a finally block. Changed-check refusal itself passed review. The repaired
candidate f95b01c4729431ba51c65837cca9451f89e2450b64c5aa8c5cd203efaee9f418
passed that refusal and all four persistence-cancellation adapter cases. The older
candidate also passed the latter; no prior cancellation failure is claimed.

The incremental fixture reviewer approved both repairs with no open finding and
independently confirmed timeout cleanup kills/reaps the owner and closes the PTY.
The parent reran all four persistence-cancellation cases plus resumed authority
and retained integration successfully against candidate f95b01c4. The specific
persistence diagnostic is now required after a captured event cursor. All three
Python files parse and the diff whitespace check passes.

## Independent runtime specification review at 105c250

Verdict: changes required. Findings recorded before repairs:

- P1, ORCH-007: after restart and reconciliation, a new assignment pumps an older
  retained queue without /agents-resume. Reproduced through the production terminal.
  Add an explicit recovered-queue pause independent of the shutdown flag.
- P1, ORCH-006: pump reserves Preparing records before launch registration;
  cancel_all can cancel/drain between those steps, then an unchecked launch can
  overwrite Cancelled with Running. This is a source-proven interleaving, not a
  reproduced production stress failure. Serialize registration with cancellation
  and preserve durable admission/cancellation gates.
- P2, ORCH-001: start_validation checks capacity outside its update, so a background
  pump can reserve the final slot before validation still enters Validating.
  Integration already checks capacity inside its update. Fix validation atomically.
- P2, ORCH-003: a stale snapshot replaces a returned role verdict, findings and
  explanation before storing the receipt. A production probe returned a unique
  finding absent from input and found it absent from the entire retained record.
  Preserve the original role response; record runtime rejection separately.

A further budget observation recorded two subscription process launches but only
one prompt and one charged invocation with an allowance of one: role backend open
precedes admission. No extra prompt or model invocation was observed. Apply the
same preflight used for workers before starting an unavailable role backend.
The runtime source reviewer changed no files and examined clean commit 105c250.

### Added production falsifiers

All three new production cases fail on the reviewed 105c250 binary, copied with
SHA-256 ad5e2b32799657d4fa35f791d366887745c1353679fe2229169227c89addf9ec:
stale_role_retention detects replaced original response fields; shared_limits
detects the unavailable advisor process launch; recovery detects a new assignment
releasing retained work without /agents-resume. The Python fixture parses. Runtime
repairs and deterministic concurrency regressions are in progress; these failures
are editing-time demonstrations, not new passing or formal Cairn evidence.

### Incremental regression review before repair

Fixture review found that the new stale-response retention cases replaced the
earlier stale clear-advisor case. Keep both: findings retention alone does not
prove stale approval cannot make work ready. The recovery case also submitted a
new assignment without proving it was accepted; a rejected command could satisfy
the queue assertion. Require a new retained assignment before testing the pause.
The backend launch assertion and targeted cleanup passed this review.

The reviewer approved both repairs. Stale-clear advisor coverage now runs on all
four adapters alongside original findings retention. Recovery requires exactly one
new record with the expected objective. On immutable candidate ad5e2b32, those
acceptance assertions passed and the retained-queue assertion still failed, which
confirms the observed defect follows an accepted command. Syntax and whitespace
checks passed; repaired runtime production passes remain pending.

### Repaired candidate production verification

The parent copied the worker's rebuilt binary before running tests, with SHA-256
fd5821f5abbb8723df4e18eede9cb68fac6f4526ca8670bd45dd5e90c267f57e.
All seven ORCH, seven SUB and six VERIFY production requirements passed on that
immutable candidate. This includes the new accepted-command recovery refusal,
all sixteen stale-role cases, exhausted backend launch refusal and cancellation
during failed persistence. These are editing-time checks. Independent runtime
source re-review, quality review, formal receipts and installed release checks
remain pending; production passes alone do not close the source review findings.

### Source re-review at cc702ba: batch cancellation finding

The independent reviewer confirmed the original direct repairs, passed both new
Rust regressions and independently reproduced the corrected recovery, original
response retention and exhausted backend launch behavior. One source-confirmed
ORCH-001/006 defect remains: pump reserves all eligible assignments as Preparing,
then exits its launch loop on the first launch error. If individual cancellation
cancels the first reserved assignment before registration, that launch correctly
refuses, but later reserved assignments receive neither a task nor an interruption
guard. They remain Preparing and consume capacity indefinitely. The existing
single-assignment global-cancellation test does not cover this case. Add a
deterministic two-assignment regression that cancels only the first reservation
and requires the unrelated second assignment to acquire a live owner. This finding
is recorded before repair; no production stress reproduction is claimed.

The worker demonstrated the new deterministic two-reservation test fails at
cc702ba because the first cancelled registration aborts resume_queue before the
second registration. Commit 669b633 repairs the batch loop and passes that test
and all 57 library tests. The existing global-cancellation regression caught an
overbroad error handler during editing; it passes again after the handler was
corrected. Independent specification re-review remains in progress. Separately,
the parent completed all seven ORCH, seven SUB and six VERIFY production cases
on exact cc702ba binary 54139d61; those passes did not detect the batch defect.

Specification review approved clean 669b6338663e9c47408bcd7767e954f8586ab491,
independently passing both cancellation regressions and all 57 library tests.
The parent copied its exact debug binary with SHA-256
f3c383120dc7f8ec03132939fd8bd692754644e78b906e9199def9841dd31579.
ORCH-001/006/007 and SUB-004/006/007 passed again on that candidate. Explicit
inherited-descriptor, registry, queue, output-limit, terminal-screen, steering and
cancellation drivers passed. The worker reports final formatting, Clippy and
189 Rust tests passed, with six opt-in cases ignored. Quality review is in progress.

The retained live-Oracle validator was run and failed because its historical input
digest is stale. No live provider call was made. This commitment uses controlled
production transports and does not refresh the earlier live-provider evidence;
that limitation is not represented as a passing live check.

## Independent runtime quality review: interrupted integration

P1 finding recorded before repair at 669b633. Cancellation now changes Integrating
to Cancelled, and finish_job handles a cancelled integration before its integration
error branch can set recovery_pending. The interruption guard also checks the
Integrating status that cancellation has already replaced. Integration applies
the patch to the parent before awaiting metadata reconciliation and capture, so
cancellation can leave changed or partial parent files without requiring explicit
inspection. Restart sees Cancelled rather than an uncertain integration. The
prior error settlement preserved that uncertainty. Preserve uncertain integration
and parent recovery_pending durably before draining, including forced-abort
fallback, and demonstrate cancellation after a parent effect with a deterministic
regression. This is a source-confirmed interleaving; no terminal reproduction has
yet been claimed. Quality review remains open and the candidate stays unchanged.

### Quality verdict and measured tradeoffs

The final quality verdict requires that one integration-cancellation fix and
identifies no other concrete defect. Independent library and orchestration-state
tests passed (57 and 6). The baseline-to-669b633 quality delta remains nonzero:
65 regressions, 31 preexisting-worse, 34 new-symbol and 16 gating findings. This is
not a clean quality-gate claim or a blanket acknowledgment.

The reviewer examined the real added complexity in pump and finish_job and tied
the cancellation defect to missing behavior coverage. Record construction in
start_after, ordered capture/check/capture in collect_current_evidence and the
receipt-before-staleness order in role_receipt remain cohesive operations. The
existing Oracle and reviewer transports retain different policies; generic
transport consolidation is not required here. Several dead-code flags are tests
that ran or used types; tiny iterator clones and same-name run/control locators
do not establish harmful duplication. Two lifecycle fixtures share setup, an
accepted two-case tradeoff. Required recovery and terminal display changes account
for their recorded growth. No broad refactor is warranted by these measurements.

The exact test-gate replacement now lists all 48 potentially untested symbols
without truncation, alongside 17 Rust obligations. Many are test functions or
adapter callbacks exercised by the production drivers; this static-map limitation
does not excuse the missing integration-cancellation test. That repair is now
authorized within the existing decision and will receive both re-reviews.

### Integration repair verification and healthy-admission regression

Commit 8af2e80 passes the three new lifecycle regressions, 60 library tests,
192 full-suite tests, formatting and Clippy (worker results). The parent ran
ORCH-002/006/007 and SUB-003/004/005/007 successfully on its exact binary, SHA-256
396b69cac37cf5806b1fd62d160cc046b41fb4273806f7ab026a677c556b71f4.

Re-review and implementer analysis identified an overbroad repair before further
changes: setting global recovery_pending during healthy integration rejects
model, backend and tool admissions from unrelated children that remain active.
Durable Integrating before effects is required; a global recovery hold throughout
healthy integration is not. Preserve recovery plus Uncertain atomically before
cancellation drains the owner, and retain interruption/restart protection, while
allowing unrelated children during healthy integration. Replace the admission
test's overbroad global-hold assertion with a real unrelated model admission;
keep both real-parent-effect cancellation regressions. This records the newly
introduced behavior error before its repair, rather than relaxing the recovery
requirement. No timing-based terminal reproduction is claimed.
