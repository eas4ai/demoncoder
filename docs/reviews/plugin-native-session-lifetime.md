# Native host session lifetime

Status: verified synchronous native command prerequisite. Fresh specification
and quality reviews pass. The lifecycle item and skills-plugins-hooks commitment
remain incomplete.

## Observed missing behavior

NonToolPlan initially rejects SessionStart. After enabling only observation-plan
construction, the production outer session boundary still fails to execute its
registered confined startup command when no prompt has been submitted. These
failures are retained in `/home/shawn/demoncoder-check-tmp/` as
`native-session-lifetime-red-constructor.log` and
`native-session-lifetime-red-boundary.log`.

The initial production control now passes in
`native-session-lifetime-green-initial-5.log`: one test executes actual confined
startup and shutdown commands without a submitted prompt. This is an intermediate
result; final cancellation, deadline, replacement and recovery evidence follows.

## Chosen boundary

The existing durable ledger owns a host lifetime independently of worker turns.
Actual outer startup and termination provide the facts. Resource close during
turn cancellation, failure or provider replacement does not create another end.
The outer runtime retains eventual termination even if the inner provider changes;
changed identity or policy suppresses stale command effects with a diagnostic.

Startup observation has a whole-boundary cap of 30 seconds; end observation has
five seconds, including waits and cleanup. No task/delegation budget is created,
reset or borrowed. New session command authority cannot reopen held or stopped
work or grant a model call. Non-command session handlers remain unavailable even
if a task allocation happens to exist.

## Verification obligations

Observe actual command effects without any prompt, exact lifetime ownership,
startup versus resume, resource close versus end, provider replacement, queued
shutdown and cancellation, stalled handlers and owned descendant cleanup.
Exercise failure, closed output and persistence holds, old records and uncertain
outcomes without replay. Existing native failure, prompt/Stop and external
lifecycle behavior must remain compatible. The results and independent specification
then quality reviews are recorded below.

## Intermediate failure demonstrations

The retained REDs expose four concrete failures: replaying a settled startup
(`native-session-lifetime-red-replay.log`), losing the lifetime on queued shutdown
(`native-session-lifetime-red-queued-shutdown.log`), executing a replacement end
command absent from startup policy (`native-session-lifetime-red-replacement.log`),
and claiming unapplied startup context was applied
(`native-session-lifetime-red-context.log`). These are development probes, not
Cairn evidence receipts.

Intermediate passing controls are in `native-session-lifetime-green-replay.log`
(one test), `native-session-lifetime-green-replacement.log` (four tests), and
`native-session-lifetime-deadline.log` (two tests). They cover exact settled-owner
replay rejection, actual no-prompt commands, cancellation with queued Submit,
queued shutdown, changed hook configuration, unchanged task counters after task
time expiry, and bounded cleanup of a shutdown command and detached descendant.
Final combined results and independent reviews follow below. Filenames alone
are not result evidence: the earlier `native-session-lifetime-green-diagnostic.log`
contains a failed intermediate fixture run and is not a passing control.

## Independent review findings

Initial specification review identified two defects. An outstanding command
sender reservation can keep the shutdown drain waiting before the observation
deadline starts. Replacing the native provider with an external provider, or one
without the old end plan, can retain the end fact but omit the required unavailable
observation diagnostic. Both now have fixes and passing production probes; fresh
source re-review confirmed them. Final verification and both independent reviews
now pass. A further probe caught explicit Shutdown being relabelled UiClosed
from final receiver state. The code now records the actual selected cause.

The retained `native-session-lifetime-red-held-permit.log`,
`native-session-lifetime-red-removed-plan.log`, and
`native-session-lifetime-red-end-reason.log` demonstrate these defects.
`native-session-lifetime-green-spec-fixes.log` passes all fourteen integration
cases, including held producer permits, removed native end plans, external
replacement, and unchanged-policy controls. The command receiver is dropped
after rejecting buffered work, without waiting for producer-held reservations.
Pinned end-policy validation also runs when no replacement plan exists.

The initial full regression stopped on a new resume fixture with mismatched saved
and selected provider identities: 342 passed and one failed in one suite. The
second stopped on a new fixture trying to register native asynchronous-rewake,
which the existing source contract correctly rejects: 514 passed, one failed,
13 ignored across 19 completed suites. These partial runs are retained as
`native-session-lifetime-full.log` and `native-session-lifetime-full-2.log`.
Neither is final passing evidence; fixture corrections preserved validation.

The third full regression passed 866 tests with zero failures and 17 ignored
across 44 suites; summed suite time was 546.27 seconds. Raw output is
`native-session-lifetime-full-3.log`, SHA-256
`cf40eae950ea74403e169543df2e2218148b772b8f923b64c41588f70ceb7055`.
This verifies the candidate before the final active-turn end-cause correction;
the final run below verifies that production change.

## Supplemental direct evidence

Four additional production probes pass individually:
`native-session-lifetime-startup-cap-4.log` exercises the shared startup limit
across two sequential commands and observes cleanup before Ready at 27.12 seconds;
`native-session-lifetime-failure-followup.log` observes a failed first turn and a
successful second turn within one lifetime;
`native-session-lifetime-durable-restart.log` interrupts an actual command after
its effect, drops and reopens the durable runtime, and verifies held recovery
without replay or an invented old end;
`native-session-lifetime-persistence-failure-2.log` damages durable state and
observes shutdown without end-command effects or later allocation admission.
Fresh specification review inspected both their source and their raw passing logs.

The active-turn cause issue has a typed correction: channel closure is distinct
from explicit Shutdown through native turns and their shared wrappers. Shutdown
consumed during failure observation retains its selected cause with the existing
outer lifetime, leaving the original provider error unchanged. The retained
`native-session-lifetime-red-active-close-2.log` and
`native-session-lifetime-red-failure-shutdown-3.log` expose both incorrect reasons.
Their `green-active-close.log` and `green-failure-shutdown.log` counterparts pass
one test each. `native-session-lifetime-cancelled-followup.log` also passes: a
cancelled first turn and successful later prompt retain one actual lifetime.
Fresh source re-review confirmed the correction. The final all-target run passes
869 tests with zero failures and 17 ignored across 44 suites (573.96 summed
suite seconds). Raw `native-session-lifetime-full-4.log` has SHA-256
`37a2937cdf5831650ad8d7bce86a3dc7bab7cdf0d8036949a69383178840a663`.
Parent inspection independently reconciled the raw counts. These development
checks are not Cairn evidence receipts. Fresh independent quality review also
passes for this prerequisite.

## Final-source lint and static checks

`cargo fmt --all -- --check` and all-target Clippy with warnings denied pass
(`native-session-lifetime-fmt-check.log` and `native-session-lifetime-clippy-3.log`).
Clippy ran with its compiler-cache wrapper disabled for that invocation after a
cache connection failure. No shared cache server or warning policy was changed.

Qualified edit checks for the outer driver, native control, wrapper, selected
cause and failure control report zero incompatible callers. Generic symbol names
make some baseline signature counts imprecise; Rust compilation and behavioral
checks provide the stronger caller evidence.

Final static diagnostics remain nonzero, not passing gates:
`native-session-lifetime-quality-delta-final.log` reports 166 rows, including
45 gating findings (25 churn, nine duplication, four reused-helper similarities,
four verbosity increases and three complexity increases). The test gate reports
43 affected test obligations and 474 symbols without mapped coverage across
24 changed files. Full Rust execution does not establish unrun Python or live
qualification. Fresh quality review assessed these findings; no baseline
or suppression was changed to remove them. Added complexity keeps eligibility,
settlement and inspection in their owning layers. Typed lifetime and turn updates
share structural patterns but have distinct authority. Small mutex accessors and
test doubles do not justify coupling unrelated subsystems; substantive fixtures
exercise different process, replacement and recovery behavior. The reviewer found
no blocking maintenance issue. Future expansion must preserve dispatch cohesion.

## Independent review and production self-audit

Fresh specification review passed in
`/home/shawn/demoncoder-check-tmp/native-session-lifetime-spec-review.md`.
The initial failed review is retained separately. Fresh quality review passed in
`/home/shawn/demoncoder-check-tmp/native-session-lifetime-quality-review.md`.
Neither review found an unresolved prerequisite defect after the repairs.

The final production-rules self-audit found no required revision for this prerequisite:

1. The outer driver, wrappers, native turns, durable ledger and runner cleanup were mapped.
2. Changes address actual lifetime observations and the demonstrated review defects.
3. The existing ledger, event sink, dispatch, process leases and inspection helpers are reused.
4. Additive optional fields preserve old records; typed termination causes update their callers.
5. Original provider errors remain unchanged; unavailable observations retain diagnostics.
6. Exact identity, workspace, policy and lifetime checks grant no task or model authority.
7. Actual interrupted effects survive durable reopen without replay or an invented end.
8. Whole asynchronous boundaries, lock waits, cancellation and descendant cleanup are exercised.
9. The plan retains one active lifecycle item; no incomplete lifecycle item is checked off.
10. Meaningful failing controls, focused production probes, formatting, lint and full tests ran.
11. Ignored tests, nonzero static diagnostics and synchronous-I/O evidence limits are explicit.
12. All remaining lifecycle behavior stays in the developer-selected complete commitment.
13. Both independent reviews passed; final checks and static dispositions require no further revision.
14. The decision and report distinguish implemented behavior, actual evidence and remaining work.

## Limits

Explicit session-hook budgets and service/accounting ownership for HTTP, MCP,
prompt and agent runners remain required. Additional startup effects, Interrupt,
outer timeout and actual backend source events also remain in this commitment.
Configured asynchronous and asynchronous-rewake session commands also remain
unavailable in this prerequisite. Their session-owned execution policy remains
required; they must not silently become synchronous commands or borrow task
continuation. No external source event or complete five-runner lifecycle support
is claimed.

The elapsed-time probes cover asynchronous lifecycle waits, command execution
and tracked cleanup under ordinary local I/O. Existing synchronous ledger locks
and filesystem calls are not preempted by Tokio timeouts; an operating-system I/O
stall is outside what these elapsed-time results establish.
