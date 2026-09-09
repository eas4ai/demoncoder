# Lifecycle hooks

Status: Draft 2026-09-09
Prefix: HOOK

The [design](../proposals/skills-plugins-hooks.md) defines event timing, delivery
scope and import profiles. A gate runs before an action can proceed. An observer
receives an event without authority to undo it. Both are owned runtime operations.

[HOOK-001] The runtime MUST dispatch versioned, typed lifecycle events at the
boundaries listed in the complete event matrix. Each invocation MUST identify
its session, task when present, role, event, causal operation, package generation
and policy. Matchers MUST use documented event-specific fields with bounded evaluation.
Falsifier: A handler sees an event for an operation that never occurred, a child
event is attributed to its parent task alone, or a pathological matcher hangs admission.
Mechanism: Record full production event traces for success, denial, failure,
cancellation and resume; compare event order, identity and missing-event behavior.

[HOOK-002] Gate groups MUST run in a stable order captured with the task;
source-defined concurrent groups follow PCOMP-002. A blocking
result MUST dominate permission to continue. A pre-tool hook MAY propose new
arguments. It MUST NOT change tool identity. The runtime MUST validate and admit
the final arguments against the existing access policy after all rewrites.
The final-candidate and snapshot protocols in PRUN-001 and PRUN-002 govern
plugin decisions as well as developer answers.
Falsifier: A later allow overrides a deny, an allowed path is rewritten to an
outside path without another policy check, or package discovery order changes decisions.
Mechanism: Compose allow/deny/rewrite fixtures in opposite discovery orders and
test permitted and forbidden final operations through every adapter.

[HOOK-003] A hook asking for developer input MUST leave the action pending and
bind any answer to the actual event, final candidate and policy generation.
The runtime MUST accept that answer only from its developer control channel.
Falsifier: Hook stdout or model text grants approval, an expired answer admits
changed arguments, or an unanswered prompt defaults to permission.
Mechanism: Supply forged, stale, cancelled and valid answers and inspect whether
the pending operation actually executes.

[HOOK-004] A timeout, invalid response or execution failure in a gate MUST leave
the guarded action blocked with a visible reason. Observer failure MUST remain
visible without replacing the result of the observed operation. Hooks MUST NOT
erase original tool evidence or mark verification, review or acceptance complete.
Falsifier: A crashed pre-tool gate permits execution, a failed post-tool hook
hides a successful write, or a formatted message changes an evidence receipt.
Mechanism: Inject failures before and after a real fixture mutation and inspect
the retained mutation result, hook receipt and workflow state independently.

[HOOK-005] A Stop gate MAY request a bounded correction for an active task, using
the task's existing cumulative allocation and correction limit. It MUST NOT
block cancellation or shutdown, start an unbudgeted loop, or accept work.
An exhausted correction allowance MUST leave the task stopped with its unmet gate.
Falsifier: A perpetually blocking hook loops forever, cancellation invokes another
model turn, or Stop success bypasses the developer's explicit acceptance.
Mechanism: Run always-block, corrected-pass, exhausted-budget and cancelled tasks;
count actual continuations, costs and acceptance transitions.

[HOOK-006] Command handlers MUST execute as tracked, confined child processes
with bounded JSON input and the bounded event-specific output protocol in
PCOMP-002, including non-JSON worktree paths. They MUST receive only declared
environment, file and network access, including when the main session uses host
mode. Shell execution MUST be explicit. Event data MUST NOT be interpolated into
executable shell source. Cancellation MUST terminate the owned process tree.
Falsifier: A handler reads a private credential canary, event text becomes shell
code, an undeclared write succeeds, or a child survives cancellation.
Mechanism: Execute harmless access, injection, timeout and descendant-process
fixtures using the production launcher; observe effects outside the protocol response.

[HOOK-007] Prompt handlers MUST use an explicitly configured model, bounded
input and a validated verdict without tool access. Agent handlers MUST run as
bounded isolated read-only assignments. Both MUST charge usage to the owning
task or an explicitly configured session allowance. The runtime MUST report
unavailable models instead of silently changing connections.
Falsifier: A prompt gate invokes a tool, a reviewer modifies the candidate it is
judging, or a handler's model use is absent from the task allocation.
Mechanism: Drive controlled model responses with malformed verdicts, attempted
tools, unavailable models and exhausted allocations; inspect actual usage and effects.

[HOOK-008] The runtime MUST persist hook admission and outcome with existing
durable workflow state. Interrupted execution with uncertain effects MUST NOT
be replayed automatically. Hook-generated activity MUST have explicit causal
identity. It MUST NOT recursively dispatch itself without an admitted bounded policy.
Falsifier: Restart repeats a side effect whose outcome is unknown, a formatter
triggers itself indefinitely, or a child reuses its parent's gate pass.
Mechanism: Interrupt immediately before and after a fixture effect, resume,
and test recursive event sources and parent/child state separation.

[HOOK-009] Claude Code and Codex hook importers MUST translate only documented,
tested event/input/output contracts from the agreed reference profiles into the
native contract. They MUST report
unsupported fields and semantic differences, including stricter failure handling,
before activation. A package requiring unavailable semantics MUST remain disabled.
Falsifier: An importer treats incompatible event names as equivalent, changes a
script's blocking meaning silently, or reports a mocked backend-private event as real.
Mechanism: Keep separate fixtures for both dialects, including exit-code blocking,
JSON decisions, argument rewrites and unsupported response fields; compare their
actual effects with the compatibility report.

[HOOK-010] The runtime MUST implement the session and orchestration operations
needed by the full lifecycle contract: explicit tool batches, compaction, workspace
changes and team-idle transitions. Hooks MUST observe real state transitions,
with pre-action gates and post-action observations where specified. Workspace
changes MUST revalidate file access. Compaction MUST preserve durable task evidence
and active plugin policy outside the shortened model context.
Falsifier: A compaction event fires without compaction, a directory change grants
unreviewed access, a batch-complete event precedes an unfinished tool, or a team
is reported idle while one of its agents is running.
Mechanism: Exercise real compaction, workspace changes, multi-tool batches and
dependent team work; block each pre-action gate and confirm the corresponding
state did not change, then permit it and inspect the actual transition.

[HOOK-011] The runtime MUST support asynchronous observers, one-shot handlers,
bounded context contributions and post-operation continuation decisions from the
agreed import profiles. It MUST preserve original effects and evidence when a
post-operation handler blocks continuation. It MUST persist one-shot consumption
and cancel asynchronous work with its owner.
Falsifier: A one-shot handler repeats after restart, an async response retroactively
authorizes an operation, a post-tool block loses a successful write receipt, or
hook context exceeds its limit without disclosure.
Mechanism: Exercise both dialects' once/async/context/continuation fixtures,
including restart between effect and result, late replies and exhausted limits.
