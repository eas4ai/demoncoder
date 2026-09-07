# Advanced orchestration

Status: Agreed 2026-09-07
Prefix: ORCH

The developer selected this commitment after assignable-subagents and confirmed
the following policy: independent assignments run concurrently; dependents wait
for validated integration; a failed prerequisite blocks its dependents while
unrelated work continues. An advisor reviews completed work, workers answer
findings, and an independent judge resolves disputes within two correction
rounds. Integration still requires an explicit developer command.

## Behavior

Enable orchestration explicitly with developer-selected supervision connections.
Reuse the four existing connection types, confined worktrees, shared allocation,
selected checks and reviewer. The selected reviewer supplies the advisor role;
a separately selected judge resolves disputed findings. Each role uses its own
context and retains its connection and model identity. Role messages are evidence,
never developer authority. This does not add recursive child delegation.

Assignments may name earlier assignment IDs as prerequisites. The runtime retains
waiting work and starts eligible independent work within the existing active
limit. Capture each dependent's worktree when it starts, after the developer has
integrated its prerequisites. A completed or merely validated prerequisite is
insufficient. An unavailable prerequisite leaves a visible reason for waiting.

After completed worker work, run the selected checks and ask the advisor to
inspect the actual current patch and results. A clear review and passing checks
can make the assignment ready for developer integration. For findings, retain
the worker's response and an independent judge's decision. If correction is
required, admit at most two corrective worker turns and rerun current checks and
review. Exhaustion, missing evidence, invalid role output or unresolved findings
hold the assignment. No supervision verdict can turn a failed check into a pass.

## Requirements and falsifiers

[ORCH-001] The runtime MUST retain bounded assignments with explicit dependencies
and run eligible independent work concurrently within the shared active limit on
the existing four connection types. Invalid, self or cyclic dependencies MUST be
rejected before work effects.
Falsifier: A queued assignment disappears, a dependent starts early, or admissions
exceed the active limit while independent eligible work could run.
Mechanism: Drive the real terminal and parent tools with delayed children, a full
queue and invalid dependencies; inspect admissions, worktree contents and effects.

[ORCH-002] A dependent MUST start only after every prerequisite has current
validated changes integrated by an explicit developer operation. Failed,
cancelled or uncertain prerequisites MUST block their dependents with a visible
reason while unrelated work can continue.
Falsifier: Completion prose or a clear review releases a dependent, a failed
prerequisite is silently ignored, or the dependent starts from pre-integration
content.
Mechanism: Hold integration, fail and cancel prerequisites, continue an independent
assignment, then explicitly integrate a successful prerequisite and inspect the
dependent's captured baseline and tool effects.

[ORCH-003] Completed worker work MUST receive an advisor review of the actual
current assignment, patch, source and selected check results. Advisor execution
MUST be tool-free and retain original findings, explanation and role identity.
Falsifier: Completion prose replaces source evidence, advisor output changes files,
or stale evidence makes an assignment ready.
Mechanism: Inspect controlled role requests, attempt forbidden role tools, mutate
files during review, and distinguish clean, failing and missing-evidence cases.

[ORCH-004] Findings MUST retain a worker response and an independent judge decision
before being resolved as a dispute. The judge MUST see original findings, the
response and current runtime evidence. Agent claims MUST NOT acquire developer
authority, erase earlier results or authorize integration.
Falsifier: Worker disagreement alone clears a finding, the judge sees only a
summary without the actual patch and results, or a role request merges changes.
Mechanism: Exercise upheld, dismissed, blocked and invalid verdicts through actual
role transports; inspect retained messages and absence of unauthorized effects.

[ORCH-005] Supervision MUST admit no more than two corrective worker turns per
assignment, rerun verification on changed files and retain spent rounds across
restart. Every role and correction MUST consume the existing shared deadline,
tool/native-call or backend-invocation allowance without resetting it.
Falsifier: A third correction runs, a role bypasses exhausted admission, or changed
files reuse a previous passing check or judge verdict.
Mechanism: Drive persistent findings and small shared limits, compare file effects
and receipts before and after correction, and interrupt then inspect spent rounds.

[ORCH-006] The terminal MUST expose waiting reasons, active roles, original
findings, responses, judgments and correction counts without blocking parent
interaction. Individual cancellation MUST stop the selected assignment; parent
cancellation and shutdown MUST stop active descendants within two seconds and
prevent queued work from starting afterward.
Falsifier: A waiting control freezes the editor, role events lose attribution,
cancellation affects the wrong assignment, or later queued effects occur.
Mechanism: Use delayed role responses and heartbeat workers beside parent work;
inspect labels, cancel individual and parent work, and observe effects after exit.

[ORCH-007] Durable recovery MUST preserve dependency identity, original supervision
evidence, correction counts and integration state. Interrupted execution MUST
remain uncertain until explicitly inspected and MUST NOT replay automatically.
Resumed execution MUST retain the original connection authority and allowances.
Falsifier: Restart forgets a dependency or finding, resets a correction budget,
replays an interrupted role or mutation, or fabricates a successful prerequisite.
Mechanism: Kill the production owner during queued work, supervision and correction;
resume with retained state and inspect identities, counters, requests and effects.

## Completion and boundaries

Complete when all seven requirements have current Cairn evidence, final review
has no open finding, and the installed release passes the production orchestration
workflow. Preserve assignable-subagents confinement, explicit integration and
verification contracts. Unknown backend usage stays unknown. Existing finite
record and snapshot limits remain explicit refusals rather than silent truncation.
Then take evidence-based-improvement, already selected by the developer.
