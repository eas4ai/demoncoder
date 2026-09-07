# Assignable subagents

Status: Agreed 2026-09-07
Prefix: SUB

The developer selected this commitment after verification-review-recovery and
confirmed the contract with these corrections: support subscription agents,
use Git worktrees that can be merged after validated completion, and never
inherit --yolo. A child is confined to its worktree even when the parent has
host access. Oracle approval cannot widen the child boundary.

## Behavior

The developer makes named connections available for delegation. The parent can
assign bounded work through the normal tool path, while continuing independent
work. Each assignment names its objective, supplied context, connection/model,
owned paths, checks and reviewer. Developer controls inspect, cancel, validate
and explicitly integrate child work. Two active children is the default; the
configured finite bound and remaining allocation are visible.

Use the existing native loop for API agents and the existing backend adapters
for Codex and Claude subscription agents. Authentication remains the selected
method. The backend may access its own login; child coding tools cannot access
home files or credentials. This commitment does not add recursive delegation,
dependency scheduling or an advisor/judge policy; those belong to the next
selected orchestration commitment.

## Requirements and falsifiers

[SUB-001] The parent MUST be able to delegate an explicit assignment to an
independently selected OpenAI API, Anthropic API, Codex subscription or Claude
subscription connection. The assignment MUST retain its objective, context,
owned paths, checks, reviewer and connection/model identity.
Falsifier: A connection is only a label, subscription execution switches to API
billing, or a child receives another assignment's identity or authority.
Mechanism: Drive parent delegation through production tool calls and terminal
controls on all four controlled transports; inspect requests and actual effects.

[SUB-002] Every child tool and validation command MUST use enforced worktree
confinement regardless of the parent's --yolo setting. Outside user files,
credentials and shared Git administration MUST remain inaccessible to child
mutations. An Oracle decision MUST NOT expand child authority.
Falsifier: A child moves, deletes or overwrites a synthetic home directory,
escapes through an absolute path, parent traversal, symlink or hard link, changes
shared Git metadata, or executes without the required sandbox.
Mechanism: Run hostile file and shell operations from all four child adapters,
including a yolo parent and an allow-returning Oracle. Compare outside canaries
and Git metadata before and after. Test missing confinement without host fallback.

[SUB-003] Each mutating child MUST run in its own actual Git worktree based on
the parent's captured starting content. Existing uncommitted and untracked
parent changes MUST be preserved. Child writes MUST NOT change the parent tree.
Falsifier: A child shares the parent's writable directory, starts from stale
committed files, or creating/stopping a child loses an existing developer edit.
Mechanism: Delegate from a disposable dirty repository, inspect git worktree
identity and initial files, then run independent parent and child edits.

[SUB-004] The terminal MUST show each child's assignment, connection, activity,
status, result and reported usage while the parent remains responsive. The
developer MUST be able to inspect and cancel one child. Parent cancellation
and shutdown MUST stop all active descendants within two seconds.
Falsifier: Delegation freezes the editor, output loses its sender, cancellation
stops the wrong child, or a child's process continues effects after the deadline.
Mechanism: Run delayed and heartbeat children beside parent work, inspect labeled
events and terminal output, cancel individual and parent work, and count effects.

[SUB-005] Integration MUST require completed child work, passing selected checks
and a clear review of the actual current child patch. Integration MUST be an
explicit developer operation that preserves unrelated parent changes and
rejects conflicting or out-of-ownership changes without overwriting them.
Merged changes MUST invalidate parent acceptance and require fresh verification.
Falsifier: Completion prose, stale checks or a child request alone merges work;
a conflict silently overwrites an edit; or parent acceptance survives integration.
Mechanism: Exercise failing checks, review findings, stale evidence, outside-owned
edits, clean integration and a concurrent parent conflict through the real app.

[SUB-006] Delegation MUST obey finite shared deadlines, tool admissions and
child concurrency limits without resetting spent allowances. Native model calls
and external backend invocations MUST be accounted under their stated controls.
Unavailable backend-internal usage MUST remain unknown rather than appearing as
an enforceable model-call, token or monetary cap.
Falsifier: A child admits effects after shared exhaustion, restart restores spent
allowances, or an opaque backend is reported to enforce an unsupported hard cap.
Mechanism: Use small limits across parent, children and validation; assert the
next effect is denied, deadlines stop work, and partial usage remains explicit.

[SUB-007] Durable recovery MUST retain assignment identity, worktree identity,
original results, validation, integration state and consumed allocation. An interrupted child or integration MUST be uncertain until explicitly inspected.
An interrupted child or integration MUST NOT replay automatically or become a fabricated successful merge.
Agent messages MUST retain sender and assignment and never become human authority.
Falsifier: Restart repeats an uncertain mutation, attaches to another worktree,
forgets a conflict/result, or promotes child text to a developer instruction.
Mechanism: Kill the production owner during child execution and integration,
resume and inspect records and effects, including changed worktrees and
unsupported opaque backend continuation. Require explicit reconciliation.

## Boundaries and completion

The runtime owns worktree creation, validation and integration. Child tools
cannot modify Git administration to commit or merge on their own. Read-only
system runtime dependencies needed to execute coding tools are distinct from
access to user home or other repositories. A shell working directory and a
model command judgment are not confinement.

Durable child state uses the existing private persistence discipline. Native
conversation can restore through its tested checkpoint contract. An external
backend interruption retains the result and worktree for inspection; it does
not claim restoration of opaque backend state or silently start a replacement.

Complete when all seven requirements have current Cairn evidence, final review
has no open finding, and the installed release passes the production child
workflow. Then take advanced-orchestration and evidence-based-improvement in
that order, as already selected by the developer.
