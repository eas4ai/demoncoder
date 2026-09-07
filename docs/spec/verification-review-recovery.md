# Verification, review, and recovery

Status: Agreed 2026-09-07
Prefix: VERIFY

The developer selected the remaining roadmap commitments in order on
2026-09-07. The developer confirmed this commitment's requirements and concrete policy. The later three commitments remain in
the selected sequence and will receive their own requirements.

## Proposed behavior

A coding task can stop without being verified or accepted. The terminal
shows those states separately. For acceptance, the developer selects the
checks and a reviewer connection. The runtime runs the checks, collects
the actual changes and results for the reviewer, and presents the evidence
for an explicit developer acceptance. An ordinary conversation can still
finish without pretending that code was verified.

The recommendation is an explicit acceptance workflow using the existing
session owner, adapters, tools and terminal. Automatically accepting a
reviewer's verdict would remove a developer decision but would give the
model authority over acceptance. Adding a separate orchestration service
would add storage and lifecycle owners before concurrent assignments need
them. Neither alternative is proposed here.

## Requirements and falsifiers

[VERIFY-001] The runtime MUST distinguish stopped work, verification status,
review status and developer acceptance. Acceptance MUST require the selected
checks to pass and the required review to have no unresolved finding against
the current workspace snapshot. Missing checks MUST mean unverified.
Falsifier: A model's completion text, an absent check, a failed check, a
blocked review or evidence from an older workspace allows acceptance.
Mechanism: Exercise the production terminal with a failing check, absent
checks, a passing check and an edit after passing; inspect each visible state
and attempt acceptance in each case.

[VERIFY-002] Verification MUST run through the existing tool-access policy
with cancellation, bounded output and retained original results. Each result
MUST identify the task, command, workspace snapshot, outcome and limitations.
Falsifier: A verification command escapes the selected access policy, receives
a protected credential, survives cancellation beyond two seconds, loses its
failure result or is attributed to another task or snapshot.
Mechanism: Run passing, failing, hanging and denied commands against disposable
repositories and synthetic credential canaries through the production path.

[VERIFY-003] The runtime MUST collect the actual patch, relevant source,
task requirements and verification output for a tool-free reviewer. A worker
summary MUST NOT substitute for this evidence. Missing or oversized evidence
MUST visibly block review rather than silently omit material. Review output
MUST be validated and retained as findings or a clear verdict.
Falsifier: A reviewer receives only a worker summary, executes a tool, accepts
silently truncated evidence, or malformed/unavailable review becomes a pass.
Mechanism: Capture the actual reviewer request with a controlled connection;
test contradictory worker claims, tool requests, oversized evidence and
malformed responses. Check untracked files and pre-existing changes too.

[VERIFY-004] The developer MUST be able to return retained findings to the
worker for bounded correction. Correction MUST rerun required checks and
obtain a fresh review. The proposed default is at most two correction rounds
per task, configurable before starting. Exhaustion MUST stop with findings
visible. A new prompt MUST NOT silently reset the same task's allowance.
Falsifier: A correction removes historical failures, skips verification or
follow-up review, exceeds its allowance or is reported accepted while blocked.
Mechanism: Drive fail/correct/pass/review through the real terminal, then
exercise persistent failure and cancellation in each phase.

[VERIFY-005] One durable task allocation MUST bound worker work, checks,
Oracle calls, review and correction cumulatively. The developer MUST see the
remaining deadline and enforceable limits before starting the workflow.
Calls and tool admissions MUST consume shared finite allowances. Phase
changes and restart MUST NOT reset them. Reported tokens and costs MUST be
accumulated without inventing unavailable values. A connection MUST reject
a requested hard token or monetary cap if it cannot enforce that cap.
Falsifier: Any auxiliary phase runs after exhaustion, restart restores spent
allocation, unknown usage appears as zero or an unenforceable cap is promised.
Mechanism: Use deliberately small allocations across worker/check/review
boundaries and restart, with delayed responses and missing usage fixtures.

[VERIFY-006] Recovery MUST restore conversation, task states, results,
findings, allocations and scoped developer decisions from a private,
versioned durable record. Persist an operation's admission before execution
and completion before reporting success. Incomplete operations MUST be
shown as uncertain. They MUST NOT automatically replay. The developer MUST
inspect and explicitly reconcile them before continuing affected work.
Falsifier: Restart repeats an uncertain mutation, forgets an answered scoped
decision, executes an unanswered request, changes workspace authority or
reports an interrupted action as complete.
Mechanism: Kill the production process before execution, during a mutation
and after a durable result; restart and inspect canary effects, decisions and
conversation. Exercise corrupt/truncated records, concurrent opens, disk
write failures and workspace changes while the application is closed.

## Implementation boundaries

The existing session owner remains responsible for task transitions. Native
model loops and external backend loops keep their declared owners. Extend
adapter capabilities for restoration and enforceable allocations; never
claim opaque backend state was restored without testing it. An unsupported
operation stops with an actionable capability explanation.

Use one authoritative private session record with explicit schema version,
exclusive writer ownership and durable transition writes. Derived terminal
views and reviewer evidence do not become competing stores. Bound retained
data and report storage failures before admitting further effects. Keep
credentials out of the record and preserve the current access boundary.

Acceptance refers to a captured workspace state, including untracked files
and pre-existing developer changes. No automatic checkout, cleanup or commit
is implied. Changes invalidate affected verification and review evidence.

The implementation must establish exact storage and adapter choices through
inspection and recorded Cairn decisions. Existing evidence logs are optional
and only serialize output; they do not currently implement this recovery
contract. The current session owner reports turn completion, while native
conversation lives in its model adapter. Those are the integration points,
not grounds for introducing another coding loop.

## Completion

Implement
and falsify each requirement through Cairn, retain current passing evidence,
review the complete change for gaps the mechanisms miss, and verify the
installed release. Then take assignable-subagents, advanced-orchestration
and evidence-based-improvement in order. Advisor cadence and any
critic/defender/judge policy remain explicit decisions for orchestration.
