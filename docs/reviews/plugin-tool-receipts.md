# Durable tool receipts

This prerequisite integrates invocation identity
and receipt retention through the existing tool executor and workflow store.
It does not implement plugin dispatch, runners, final-candidate policy or the
complete plugin commitment. Specification and quality reviews approved the
corrections below for this prerequisite.

## Failure demonstrated

The implementer ran `cargo test --test plugin_tool_receipts` against a real
NativeSession, ToolExecutor and runtime Store. One model response repeated edit
call `source-1`. The fixture expected `ab`, but the file contained `abb`: the
same invocation executed the effect twice. The regression failed before the
receipt integration.

## Required distinctions

A source call ID belongs to a host invocation. Retrying the same request within
that invocation must not execute it twice. Reusing a call ID in a later invocation
is new work; changing the request under the same identity must hold.

An admitted tool attempt must consume its allowance before a before-hook can
execute. A non-debiting budget precheck is insufficient: concurrent calls could
both pass it and run hooks before the final debit. Charging once at capture means
a later hook denial still consumed an admitted attempt. Receipt replay consumes
no new attempt; final tool admission must not charge it a second time.

The original result must become durable before language diagnostics, presentation
or UI delivery can await. A failed or interrupted observer cannot change a known
successful mutation into an unknown or failed mutation. Model-facing additions
need separate settlement; interrupted observers cannot replay implicitly.

Historical records without invocation identity remain readable but do not prove
that a new request is a safe replay. Tracking ordinary backend invocations must
preserve the existing allocation and delegated-backend allowance rules.

## First specification review

Two findings required correction:

1. The executor marks its local admission flag before path confinement and Oracle
   authorization. A forbidden `.git` or parent-traversal write then reaches
   presentation hooks after access rejection. Add an actual executor regression
   with a counting presentation hook, and distinguish final authorization from
   the earlier durable request reservation.
2. Final CLI checks failed VERIFY-005 and SUB-006. An early budget refusal returns
   an error before constructing the known non-executed negative tool result.
   Verification loses its failed check receipt, and the child fails instead of
   finishing with its negative tool receipt. Retain a distinct denied request
   outcome without reserving an unavailable attempt or running hooks. Identity
   collisions and unknown outcomes must still hold without replacing receipts.

The reviewer also noted that UI and Oracle waits occur after the early runtime
admission check. The correction must check the actual pre-effect ordering for
owner, deadline and recovery validity; an earlier check cannot substitute for
that boundary. This does not claim filesystem or external atomic freshness.

This review inspected source and regression tests but did not independently run
the final CLI checks. Their failures were reported by the implementer. No code
changed during the review.

## Correction verification

Known allowance/deadline denials now retain a non-executed negative result without
running hooks. Clock uncertainty, identity collisions, unknown outcomes and
persistence failures remain holds. Presentation eligibility follows the actual
effect boundary; owner, deadline and recovery checks run after UI/Oracle waits,
including before file creation. Observer admission also rechecks those conditions.

The implementer reran the unchanged failing CLI cases successfully:

- `python3 tests/verification_workflow.py --requirement VERIFY-005`
- `python3 tests/assignable_subagents.py --requirement SUB-006`

The corrected runtime unit suite passed 29 tests. Six affected integration
binaries passed 57 tests with three intentional ignores. All-target Clippy with
warnings denied, build and formatting passed. Tests include real Oracle denial
and a hold during Oracle review, forbidden `.git`/parent paths, and deadline,
recovery and child-stop changes during UI backpressure for new and existing files.

Controlled mutations removing the presentation-path guard and effect-boundary
revalidation failed the unchanged tests. The source was restored before the
passing checks above.

Specification re-review inspected the correction and the regression bodies and
approved this prerequisite with no remaining concrete findings. It did not
independently rerun these final checks. Quality review is pending; development
results are not Cairn evidence or whole-commitment completion.

## First quality review

The reviewer independently passed all 22 receipt unit tests and three adapter
wire-path integration tests. It requested these corrections:

1. Observer admission checks time inside `update`, but the subsequent time
   checkpoint or store write can invalidate the clock or exhaust the deadline.
   `update` still returns success and the presentation hook runs. Use the existing
   post-persistence admission guard for observer starts. Observer outcomes must
   remain writable after expiration. Add a regression that withholds presentation
   when the durable checkpoint invalidates admission. The finding was established
   by source ordering, not a simulated rollback during this review.
2. The concurrent-allowance fixture has unbounded polling and a blocking barrier.
   A missing hook arrival can hang the suite. Bound the coordination so a broken
   protocol fails the test.
3. The LSP fixture controller lacks failure-path cleanup and its initial blocking
   read has no timeout. Bound startup and clean up the owned process if assertions
   fail.

The reviewer found the executor's ordering cohesive despite its increased
complexity. Explicit EventSink forwarding preserves boundaries; model, backend
and command admission have different accounting. Fixture similarity and graph
misses did not justify broad deduplication or removing exercised methods. Churn
counts alone did not reveal a defect. The concrete observer ordering mismatch
above required the correction below.

## Quality correction verification

Observer starts now use the existing post-persistence admission guard; outcomes
use ordinary persistence so expiration cannot discard completed evidence. The
new executor regression simulated rollback at the checkpoint after the initial
observer check. It failed on the old implementation, then passed with zero
presentations, a durable successful edit, and a retained later observer outcome.
Its test-only injection is thread-local and bound to the fixture's unique
workspace; production builds omit it.

The concurrent fixture now has bounded arrival, release and join coordination
with peer cleanup. The LSP controller uses an owned Tokio child with kill-on-drop
and bounded asynchronous startup/shutdown.

After these corrections, the implementer passed 30 runtime unit tests, including
23 receipt tests; 34 affected integration tests with three intentional ignores;
all-target Clippy, formatting and diff checks. The larger integration, build and
CLI checks above preceded this small correction and are not claimed as reruns.
The same quality reviewer independently reran all 23 receipt tests and approved
the corrections with no remaining actionable findings.

Final refreshed Ripwire reports still exit 2 for quality delta and 4 for test
obligations. The 36 gating rows retain the reviewed disposition above. New report
differences reflect the corrected observer guard, bounded fixture coordination
and the exercised rollback test being missed by the graph. The complete test
listing contains 137 symbols and 26 test paths; broader installed/live obligations
remain for whole-commitment verification. These reports are not claimed green.
