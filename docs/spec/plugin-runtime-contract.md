# Plugin admission, state and recovery

Status: Draft 2026-09-09
Prefix: PRUN

This contract resolves the adversarial review without reducing the complete
[plugin commitment](../commitments/skills-plugins-hooks.md). Its state transitions
are part of the deliverable, not implementation suggestions.

[PRUN-001] The runtime MUST apply the final-candidate admission protocol below
to every mutating or blocking hook chain. A rewrite MUST invalidate decisions
about different arguments. Revalidation MUST NOT replay an effectful handler.
Falsifier: A policy permits one path, a later hook rewrites it to a forbidden
workspace path, and ordinary workspace access alone admits the operation.
Mechanism: Compose a generated-file policy and a path rewriter in both orders;
inspect actual denied and permitted writes, handler effect counts and rewrite cycles.

[PRUN-002] Every gate decision MUST bind to the inspected candidate, input set,
policy and relevant external preconditions described below. Changed inputs MUST
invalidate the decision before the guarded transition is published.
Falsifier: A delayed pass releases revision B after the handler inspected A,
including a new previously absent file, a changed permission or an expired remote token.
Mechanism: Delay command, prompt and agent gates across changes and unchanged
controls; inspect their actual snapshots, admission preconditions and stale-result holds.

[PRUN-003] The developer MUST have a non-hooked quarantine and recovery control.
Recovery MUST respect managed-policy authority without allowing the failing hook
to veto its own quarantine. Quarantine MUST NOT approve affected work.
Falsifier: A crashing ConfigChange gate prevents quarantine, plugin text invokes
the recovery control, or removing a gate silently marks the blocked action allowed.
Mechanism: Quarantine a perpetually failing configuration handler from developer
controls; attempt the same from every agent/tool/channel origin and test managed policy.
Recover a task with a settled mutation and active children through the replacement
transaction below; interrupt every boundary and verify generation and allowance ownership.

[PRUN-004] Plugin state MUST use the versioned storage and activation transaction
below. Activation, migration and rollback MUST preserve referenced old state.
Falsifier: A v2 migration changes a database still mounted by a v1 task, a failed
migration destroys the working version, or rollback silently discards newer writes.
Mechanism: Hold v1 work open during update, crash each transaction boundary,
exercise competing writers, rollback and removal, and inspect both state generations.

[PRUN-005] The bundled Best Practices and Cairn packages MUST implement the
specific policy and verdict mappings below. A demonstration-only fixture rule
MUST NOT substitute for those obligations.
Falsifier: A package passes its example while allowing a real failed or stale
check to discharge its completion gate, or it treats a Cairn execution error as Done.
Mechanism: Exercise normal dirty coding projects with failed/current/stale checks,
review findings, exhausted allocations and every referee verdict through installed packages.

[PRUN-006] Monitors and channels MUST implement the delivery and overflow state
machine below. Loss, duplicate suppression and recovery MUST remain durable and visible.
Falsifier: An overflowed line-only monitor cannot resume, replay repeats work,
or a reconnect acknowledges an event before the application durably retained it.
Mechanism: Overflow both source classes, resume them, kill the owner around
acknowledgment and delivery, and inspect retained gaps, deduplication and resulting work.

## Final-candidate admission

The immutable admission key contains event/operation ID, canonical tool name,
canonical arguments digest, package and policy generation, role, workspace
identity, observed-input revision and any external precondition tokens.

1. Capture the request and resolve applicable handlers. A handler is a transformer,
   a decision gate, an observer, or a legacy combined handler. The activation
   record names its class and access. Undeclared legacy handlers are combined
   and are never assumed safe to replay.
2. Run transformers against bounded candidates before final decision gates.
   Native transformers cannot grant permission. Legacy combined handlers may
   return decisions and rewrites; a deny is retained, but an allow is valid only
   for the exact candidate that handler saw. Recompute matchers after a rewrite
   when their declared input includes changed fields.
3. Freeze the final candidate. Run every applicable decision gate against it.
   A result from an earlier combined handler counts only if its admission key
   matches this final candidate. A changed key requires a separate declared
   read-only decision endpoint, or a visible `needs-revalidation` hold. The
   runtime never silently treats the old pass as current or reruns side effects.
   The developer can start a new explicit attempt after inspecting such a hold;
   that does not automatically approve the operation.
4. Resolve developer questions against the frozen key. Any later rewrite,
   settings change or observed-input change cancels those pending answers.
   A deny wins across all decisions. A handler cannot turn a deny into a rewrite
   and retry indefinitely.
5. Validate the final schema, host access, allocation and preconditions. Persist
   admission before effects; execute at most once for that operation ID. Persist
   the actual result before post-operation handlers or display transformations.

Transformations stop after four candidate revisions or a repeated candidate
digest, whichever comes first. Exhaustion leaves a visible hold; it never admits
the last candidate optimistically. A handler that requires concurrent startup
uses the dialect's concurrent group protocol in the compatibility contract;
concurrency does not make decisions about different candidates interchangeable.

## Candidate freshness

Decision gates declare their read set: workspace paths/globs, absent paths whose
creation matters, plugin data revision, configuration and external inputs. The
default for a legacy gate is the entire admitted workspace snapshot, not an empty
set inferred from missing declarations. Protected contents remain excluded.
Directory membership, file kind, symlink target, file bytes and access-relevant
metadata (ownership, mode and applicable ACLs) participate in the revision. A
missing or oversized snapshot blocks inspection; it is not a clean result.

Command and agent gates inspect an immutable admitted snapshot built from the
current dirty workspace, including untracked files. Prompt gates receive its
bounded evidence with path/revision provenance. The main worktree cannot substitute
for a hook agent's snapshot merely because the agent is read-only. Code-transforming
handlers execute before that snapshot is captured; they are not decision gates.

Before releasing a result, rescan the read set and compare its revision. For a
host mutation, compare preconditions again inside the host's serialized mutation
boundary. For task completion or acceptance, compare before publishing the state
transition; later detected changes invalidate that state as under VERIFY-001.
Concurrent outside processes cannot be locked by an application mutex. A gate
requiring atomic external-file or remote-state preconditions needs an operation
that supports compare-and-swap or an equivalent provider transaction. Otherwise
the application reports that stronger guarantee unavailable and holds that action;
it does not claim the precheck eliminated all external races.

Remote gates declare revision/ETag/transaction tokens when available and a finite
validity deadline. Token expiry, changed input or inability to confirm a required
precondition leaves the action held. A time-based observation without a transaction
can be labeled as such, but cannot discharge a policy requiring atomic freshness.
All retries and refreshed inspections consume the original allocation.

## Configuration recovery

Normal enable, update and ordinary settings changes invoke the currently active
generation's ConfigChange gates against the proposed configuration digest. The
new generation never approves its own installation. Quarantine is a separate
developer control in plugin details and a `plugins quarantine <identity>` host
control operation. It executes no package code, including ConfigChange hooks.

Quarantine atomically blocks new admissions, cancels owned execution and records
the actor, reason, affected generations and interrupted operations. It keeps
unsettled effects uncertain and required gates held. A developer may quarantine
managed code to stop it, but only the configured managed-policy authority may
remove that required policy or authorize a replacement. An ordinary developer
can inspect/export the hold and exit; quarantine does not grant a bypass.

The host control channel authenticates its local developer origin. A tool invoking
a lookalike command, a model message, hook output or a channel message cannot use
it. Repair loads a replacement in a code-free validation mode; the authorized
policy change names the old and replacement digests. Installing repaired code
does not resume tasks pinned to the old generation. Recovery requires the separate
developer action **Continue in a replacement task**, defined below. It never
reuses an old approval by omission.

### Replacement after policy repair

The old task keeps its generation, instructions and records. It remains held
until the developer selects a validated replacement code/state/policy tuple and
confirms the recovery preview. The preview identifies the old task and children,
completed effects, uncertain operations, retained worktrees, invalidated decisions
and the remaining allowance. This is not the ordinary reload action and cannot
be invoked by model text, a hook, a channel or an automatic retry.

Recovery uses one durable transaction ID and these transitions:

1. `prepared`: stop new admissions in the affected task tree; cancel and join
   owned runners. Reconcile every operation with uncertain effects before
   proceeding. A failed reconciliation keeps recovery held. Capture the current
   candidate and completed operation receipts; do not revert completed writes.
2. `validated`: stage the replacement task with a fresh instruction assembly
   and one coherent generation tuple. Carry the developer's task objective,
   current authorized workspace and cited effect history. Do not copy the old
   model context as active instructions. No old approval or gate pass discharges
   a new transition. Verification/review records remain inspectable but satisfy
   new completion only after the existing freshness and policy checks accept them.
3. `activated`: atomically mark the old task `superseded-by:<new-task>` and make
   the new task the sole owner of future admissions. If the transaction has not
   activated, the old task remains held and the staged task cannot execute. A
   crash after activation restores that same new owner; replaying the developer
   request returns the existing transaction result rather than creating a third task.

The replacement shares the original cumulative allocation ledger, including
costs, corrections and time already spent. It does not receive a fresh allowance.
An expired deadline or exhausted ledger leaves it held; only a separate explicit
developer allocation change can extend it. Charge recovery work to that ledger.
Settled operation IDs and receipts transfer as history, never as a replay queue.
Only remaining work can create new operation IDs, under fresh admission.

Each affected child is cancelled and retained as a historical assignment. A child
with unfinished work may receive a linked replacement through the same protocol,
with its existing ownership and remaining child/parent allocations. Keep its
worktree and completed effects isolated; do not auto-integrate them. Replace
task/child ownership atomically so the old and new owners cannot write the same
worktree concurrently. Background services restart or reuse only under the
ordinary complete-identity rules. Retain referenced old code and state for
inspection; PRUN-004 supplies the replacement state generation and migration.

If recovery requires a state migration, the preview names its command digest
and staged state destination. The developer-authorized recovery operation may
run that migration under host confinement without the quarantined package's
gates. Its grant covers staged plugin state only, not task workspace writes,
network effects or credentials. It consumes the remaining recovery allowance;
insufficient allowance leaves it held. This exception cannot admit ordinary
task work or execute an unreviewed migration.

Fresh plugin decisions apply to the replacement task only. Removing a policy
instead of repairing it uses the same developer-authorized transition and the
managed-authority rule. No task ever mixes its old instruction generation with
a new handler. The original task is superseded, not accepted or falsely completed.

## Mutable state and activation

State identity is `(plugin origin, package identity, workspace identity, role,
state generation)`. Code generation and state generation are distinct recorded
values. A task pins both. Shared state across roles/workspaces requires an explicit
grant; names alone cannot cause sharing. Credential material is in the host's
credential store, not copied into plugin state or migration snapshots.

By default, a changed package receives a new state generation. An update waits
for writers to the old generation to quiesce, takes a consistent copy, and runs
any explicitly declared migration against the staged copy. Without a migration,
the copy preserves existing bytes. Old tasks may resume against their old copy
after staging; their later writes are retained but are not silently merged into
the new generation. Plugin details show divergent generations and offer an
explicit merge/export operation supported by that plugin. If the plugin requires
one shared writable database, update instead waits until all old users release
it; there is no simultaneous incompatible access.

The transaction is `prepared → migrated → validated → activated`, with each
transition durable. Migrations run as bounded admitted commands, once per
transaction ID. Unknown effects require inspection, not automatic rerun. Only
activation publishes the code/state/configuration tuple atomically. A failure
before activation leaves the old tuple active. A crash after activation restores
the published tuple, even if cleanup did not finish.

Rollback selects an intact previous tuple after quiescing affected work. Newer
state remains retained and visible; rollback does not erase or silently merge it.
Removal distinguishes deactivate, uninstall code and explicitly delete retained
data. Referenced generations cannot be garbage-collected. Deleting retained data
requires a developer action that identifies the affected work and backups.
Service reuse requires identical code/configuration, workspace/role authority,
state generation and credential binding. Secret revocation overrides old pins:
stop new authenticated work immediately and reconcile already admitted effects.

## Bundled workflow obligations

The Best Practices package reads the project's authoritative BEST_PRACTICES.md
for instruction context. Its executable policy is a developer-approved declaration
of checks, their influencing inputs, required review and protected completion
transitions, stored with its activation policy. Prose is not compiled into
invented executable rules. Missing executable policy is `not configured` and
cannot be advertised as full enforcement.

Its command gates enforce these actual obligations at normal task completion:
selected checks executed and exited successfully; evidence describes the current
input revision; required review is current with no unresolved finding; unfinished
work items remain visible; cumulative correction/allocation exhaustion is honored;
and a model completion cannot set developer acceptance. A managed todo list has
exactly one in-progress item while work is active and none when completed. Existing
runtime receipts and typed workflow state supply these facts, not a marker file
or the worker's summary. Ordinary informational turns are explicitly outside this
coding-completion gate. The UI distinguishes this declaration from prose-only rules.

The skill and tool-free review cover judgment: understanding scope, coherent
changes, maintainability, sound tests and whether security/performance choices
are adequate. They produce review findings, not claims of mechanical proof.
Fixtures must falsify each executable obligation in an ordinary temporary project
without special filenames that are meaningful only to the example.

The Cairn package exposes `wake`, `check`, `decide`, `backlog`, `escalate`, `answer`
and decision supersession through ordinary admitted tools. It follows the installed
referee's versioned CLI contract; it does not infer completion from exit code zero
alone. Read-only wake output must be associated with the current committed inputs.

| Referee result | Required runtime behavior |
|---|---|
| Done | Discharge this package's referee gate for the inspected revision; retain ordinary verification/review/developer acceptance requirements |
| Resolvable | Show the exact named next action; keep completion held and let admitted worker work perform that action within the same allowance |
| Escalate | Surface the existing question and hold dependent work; only developer-origin `ok`/`instead` closes a decision |
| Pending `ask` reply | Permit the named explanation action only; neither an explanation nor silence authorizes implementation |
| Execution error, unknown version/output, damaged record | Show failure and hold the gate; never turn it into Done |
| Candidate changes after wake/check | Invalidate the referee pass and obtain fresh applicable evidence |

No command that posts, accepts, merges or otherwise changes authority is authorized
merely by this mapping. Existing developer authorization and the referee's named
action still apply. Both workflow packages ship in full and are optional to enable.

## Monitor and channel delivery

Each source has durable states `running`, `overflowed`, `recovering`, `stopped`
and `failed`, plus source generation, receive sequence, retained queue, acknowledged
cursor when available, delivery IDs and gap records. Queue acceptance is separate
from delivery to the model and separate again from admission of resulting work.
Retain deduplication IDs until their delivery/work receipts can no longer be retried;
reaching the retention bound pauses intake instead of forgetting retry identities.

For a line-only monitor, a full queue stops the producer and records the first
unretained sequence with `lost-count: unknown`; bounded stdout buffering is not
a replay guarantee. Keep the retained prefix inspectable. The developer can
drain/discard that prefix explicitly and choose **Restart with acknowledged gap**.
Restart creates a new source generation and emits the durable gap marker before
new messages. The package may provide an admitted snapshot command to reconstruct
current state; without one, the missing history remains unknown, never recovered.

For a replay-capable source, stop acknowledging at overflow and pause/disconnect
intake. After queue space is available, replay from the last durably accepted
cursor and suppress duplicate IDs. A cursor rejected by the source becomes the
same visible gap/recovery choice as a line-only source. A message is acknowledged
only after durable queue acceptance; it is not thereby executed or accepted by a
developer. Expired queued work is shown and discarded explicitly, not delivered
as a new instruction. Reauthentication never changes the bound source identity.

The delivery guarantee is at-least-once intake with deduplicated local admission
when the source supplies stable IDs. It is not exactly-once remote effects.
Unknown effects from admitted work retain ordinary reconciliation requirements.
Restart resumes the durable recovery state and does not automatically acknowledge
a gap or restart an untrusted producer.
