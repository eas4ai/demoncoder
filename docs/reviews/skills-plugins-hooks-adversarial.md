# Adversarial review of the plugin specification

Date: 2026-09-09
Reviewed commit: 1ee77cb31d4ba2d3921c99274f47da3fbb077e73
Original verdict: Not ready for implementation planning.
Remediation: All ten specification findings addressed below on 2026-09-09.
Follow-up: [Second review](skills-plugins-hooks-second-pass.md) identifies three
remaining gaps; the closure assessment below is superseded by that review.

The complete scope remains required. None of these findings recommends an MVP,
deferring a component, or calling an unsupported feature complete. The problem is
that several contracts are missing or contradictory, so an implementation can
obey parts of the text while breaking the intended behavior.

This is a specification review, not a claim that these bugs exist in an implemented
plugin runtime. Counterexamples below are execution traces permitted or left
ambiguous by the draft. They were not run against a plugin implementation.
P1 findings need resolution before implementation planning; P2 identifies a
specific operational contract that must also be resolved before delivery.

## Findings

### F01 — P1: The manifest-selection rule drops valid Codex overlay components

Location: [design, Package and skill contract](../proposals/skills-plugins-hooks.md#package-and-skill-contract), lines 40–46; EXT-001.

The draft requires choosing one dialect when multiple manifests exist. A valid
portable package can instead contain root identity, skills and MCP configuration
plus OpenAI-specific hooks in `.codex-plugin/plugin.json`. Selecting only the root
loses the hooks; selecting only the overlay loses the canonical portable components.

The current [OpenAI package contract](https://developers.openai.com/plugins/build/plugins#add-openai-specific-metadata)
defines these as layers: root identity and portable components remain canonical;
the compatibility overlay supplies OpenAI settings when an inline
`extensions.com.openai` object is absent. An inline object replaces that overlay's
OpenAI settings. This is a confirmed source mismatch, not a hypothetical future format.

Required correction: specify per-format precedence and replacement/extension rules.
Reserve developer dialect selection for actual ambiguous alternatives, not a
documented overlay. Include Claude default/custom path merge rules in the same table.

Acceptance attack: load a portable package with one root skill, one root MCP
server and one overlay hook. All three must appear exactly once. Add an inline
OpenAI extension and verify the documented replacement without losing root components.

### F02 — P1: A later rewrite can bypass an earlier plugin gate

Location: [HOOK-002](../spec/lifecycle-hooks.md), lines 19–26; design lines 166–178.

The text reruns ordinary access checks after rewriting but does not require
plugin policy gates to judge the final request. Consider a workspace where ordinary
access permits writing generated files, but a plugin gate forbids it:

1. A write requests `docs/note.txt`.
2. Gate A permits it because the target is not generated.
3. Hook B rewrites the target to `generated/config.txt`.
4. The runtime's ordinary workspace access check permits the final path.

No recorded deny was overridden, and all stated ordering and final-access rules
were followed. Gate A's intended policy was still bypassed. Binding developer
answers in HOOK-003 does not bind automatically produced gate verdicts.

Required correction: define a final-candidate admission phase. Either separate
transformers from decision gates or explicitly invalidate and reevaluate affected
decisions after any rewrite. Avoid replaying effectful handlers just to recheck
policy; separate their effect from their decision. Bound rewrite cycles.

Acceptance attack: compose this pair in both orders. The forbidden final write
must never execute, while an allowed final rewrite must execute once.

### F03 — P1: Gate results are not bound to the workspace state they inspected

Location: [HOOK-003 and HOOK-007–008](../spec/lifecycle-hooks.md), lines 28–34 and 64–81; design lines 191–207.

Generation pinning freezes plugin code. It does not freeze the candidate files.
An isolated read-only hook agent can review revision A while the parent, another
process or a formatter creates revision B. Its eventual pass can release work on
B: event identity and plugin digest are unchanged, and no rule requires rejecting
that stale result. Command gates have the same gap.

The existing [VERIFY-001/003 contract](../spec/verification-review-recovery.md)
already requires current candidate evidence for acceptance. The draft does not
extend equivalent freshness checks to generic hook decisions or identify the
candidate copied into a hook agent's workspace.

Required correction: define each gate's observed input set, candidate revision,
snapshot construction and invalidation rule. Recheck the relevant revision at
the guarded transition. A changed candidate needs a fresh decision, with its cost
charged to the same allowance. Explain how hooks with remote or time-sensitive
inputs establish validity without promising arbitrary eternal freshness.

Acceptance attack: delay a gate, mutate a declared input after its inspection,
then deliver a pass. The stale pass must not release the new candidate. Repeat
with a no-change candidate to establish that valid delayed results still work.

### F04 — P1: The generic response model omits required event-specific behavior

Location: [event matrix and Handler behavior](../proposals/skills-plugins-hooks.md#lifecycle-event-matrix), lines 129–170; HOOK-006/009/011.

The draft supplies three dispositions and unnamed event-specific fields. It does
not define how required foreign responses map to them. Concrete examples:

- Claude command `WorktreeCreate` returns a path on stdout, rather than the normal
  JSON decision. `FileChanged` can return a replacement dynamic watch list.
  These need distinct decoding and state transitions. See the
  [Claude hook contract](https://code.claude.com/docs/en/hooks#worktreecreate-output)
  and [FileChanged output](https://code.claude.com/docs/en/hooks#filechanged-output).
- Codex `PostCompact` can stop continuation after compaction. The draft labels
  this only as a result observation; its explicit continuation rules concern
  post-tool handlers. See [Codex PostCompact](https://learn.chatgpt.com/docs/hooks#postcompact).
- Claude matching handlers execute concurrently, whereas the draft orders gates
  sequentially. This needs a declared semantic translation and tests for handlers
  that depend on concurrent start. See [Claude hook execution](https://code.claude.com/docs/en/hooks).

Required correction: a dialect × event × handler-type matrix defining input,
matcher, result format, exit-code meaning, timing, context effect, blocking effect,
deduplication and ordering. Include special results such as paths and watch lists.
Classify deliberate host-policy differences explicitly; do not let a generic
invalid-response rule accidentally reject a supported upstream response.

Acceptance attack: execute unchanged upstream-shaped handlers for the examples
above and verify the resulting workspace, watch list and continuation state.

### F05 — P1: Full backend event coverage has no defined interception contract

Location: [design](../proposals/skills-plugins-hooks.md#lifecycle-event-matrix), lines 156–163; EXT-007; HOOK-010.

The required matrix includes pre-compaction veto, batch boundaries and session
workspace transitions on all four connections. Observing a backend's completion
after the fact cannot veto its action beforehand. Adding a host event with the
same name is expressly disallowed when the meaning differs.

Current source confirms that the necessary bridge is not already established:
`src/adapters/claude.rs:100` initializes with `hooks: null`;
`src/adapters/codex.rs:74–91` disables backend hooks and plugins. The draft says to
preserve one execution owner, but does not define an exception for a trusted host
event bridge, a callback protocol, or how the backend is suspended for a gate.
This does not prove the backends cannot support it. It proves the promised path
has not been specified or demonstrated.

Required correction: map every required event to the actual source and blocking
protocol for each connection, including automatic backend operations. Distinguish
host-installed event forwarding from ambient plugin execution. Name the behavior
when forwarding disconnects while the backend is waiting. Resolve actual gaps
within the complete deliverable; escalate only a demonstrated external constraint.

Acceptance attack: trigger real backend compaction with a denying gate, not a
fabricated event. Confirm that compaction did not happen, then permit it and
observe its result. Repeat for batch and workspace boundaries.

### F06 — P1: A broken configuration gate can veto its own repair

Location: [EXT-004](../spec/extensions.md), lines 44–53; [ConfigChange and failure behavior](../proposals/skills-plugins-hooks.md#lifecycle-event-matrix), lines 146 and 197–202.

ConfigChange gates precede settings activation, and gate failures block the action.
The draft also requires immediate disable and says blocked work can recover when
the developer removes or replaces the policy. It does not exempt that repair
operation from the broken gate. A handler that times out on every ConfigChange
can therefore block the change needed to disable it. Conversely, bypassing all
config gates without a defined control channel can defeat managed policy.

Required correction: define an out-of-band developer recovery operation, its
authority relative to managed policy, durable record and effect on active work.
It must stop new plugin admissions without converting blocked work into approval.
State which changes invoke the old generation's gates and which can bypass them.

Acceptance attack: install a gate that times out on configuration changes, then
disable or repair it through developer controls. Recovery must succeed without
running guarded work, and the same request from a model or plugin must fail.

### F07 — P1: Immutable package versions still share undefined mutable data

Location: [EXT-004](../spec/extensions.md), [PLUG-005/011](../spec/plugin-components.md); design lines 82–84, 115–121 and 253.

Old generations remain usable by running tasks while updates activate new ones.
The design also gives plugins persistent data and agent memory. It specifies no
data schema, migration ownership, generation isolation or rollback rule.

Version 1 can be running when version 2 migrates a shared database. Version 1 then
reads an incompatible schema. Rolling back the package does not roll back that
data. A failed update can preserve the old code while destroying the promised
working generation. Pinning file digests does not solve this.

Required correction: specify mutable-state ownership, version compatibility,
concurrent writers, migration admission, recovery and removal/retention behavior.
Use isolated versioned state or a declared backward-compatible migration contract;
define what happens when old tasks still hold references. Credential rotation and
shared service state need an explicit equivalent rule.

Acceptance attack: hold a v1 task open, activate a v2 state-schema change, crash
during migration and roll back. Verify that neither generation reads corrupted
state and that the old task either works or remains explicitly held for repair.

### F08 — P1: The Best Practices gate can satisfy the spec by checking a toy rule

Location: [EXT-008](../spec/extensions.md), lines 86–94; [Workflow plugins](../proposals/skills-plugins-hooks.md#workflow-plugins), lines 235–240.

The required gate only needs to reject a documented violating fixture or an
explicit fixture policy. A script that checks whether `example.txt` contains
`PASS` meets that wording without enforcing any production coding rule. The
mechanism tests that same fixture, so it would certify the empty implementation.
The Cairn package's commands and verdict-to-runtime actions are also not enumerated.

Required correction: name the actual machine-checkable obligations, where their
policy comes from, which real runtime transitions they gate, and which obligations
remain human/model judgment. For Cairn, specify the supported commands and how
Done, Resolvable, Escalate, pending answers and execution errors affect the task.
Keep ordinary operation independent of enabling these packages.

Acceptance attack: violate a named real policy in a normal temporary coding
project and try the guarded transition. Correct that policy without changing
the gate. Test both workflow packages through their installed production path,
including disagreement between model prose and actual check/referee output.

### F09 — P1: “All features” has no fixed conformance inventory

Location: [EXT-009](../spec/extensions.md), [commitment completion](../commitments/skills-plugins-hooks.md), and design lines 310–335, 363–380.

The contract refers to agreed reference profiles, supported positional arguments,
documented discovery rules and representative real packages. The draft does not
actually identify those profiles, package revisions or the full field inventory.
A date and links to changing upstream pages do not fix the contract. The
implementation can select easy fixtures and claim their tested behavior is the
agreed profile. The no-deferral sentence does not close that loophole.

This also leaves large items such as registered app identities underspecified:
the text promises a connector binding without naming an identity-resolution
protocol or an initial real service whose authentication and tools must work.

Required correction: commit a versioned conformance inventory covering every
component, field, merge rule and event in the selected references. Name licensed
real package fixtures by immutable revision and map each feature to positive and
negative production tests across applicable connections. Specify connector
resolution and an authorized service case. Selection establishes a reproducible
baseline for full delivery; it must not remove any requested feature category.

Acceptance attack: remove one nontrivial component or field handler from the
implementation. The inventory's coverage check and production test must fail
without the implementer changing which fixtures count as representative.

### F10 — P2: Monitor overflow requires a recovery operation that does not exist

Location: [PLUG-004/008](../spec/plugin-components.md), [resource limits](../proposals/skills-plugins-hooks.md#initial-resource-limits), line 227.

Overflow pauses delivery until reconciliation, but monitors emit arbitrary lines.
No cursor, replay operation, resynchronization snapshot, manual resume control or
loss acknowledgment is defined. A log watcher can fill the queue once and remain
paused forever, or resume by silently losing the observation that mattered.

Required correction: specify behavior for replay-capable and line-only sources.
Define what is retained, whether the producer is paused or stopped, the visible
loss marker, the exact resume action and how stale queued work is handled.
Authentication does not provide reliable delivery; channels need corresponding
duplicate, acknowledgment and reconnect rules.

Acceptance attack: overflow a live monitor, observe the loss status, perform the
defined recovery and confirm later messages arrive once. Restart during that
recovery and test a source that cannot replay its lost messages.

## Review method and limits

Read all five draft documents and compared their promises with the agreed
verification, assignment, settings and language-service contracts. Inspected the
two backend initialization boundaries after the graph service returned a closed
transport. Checked current official Claude and OpenAI package/hook documentation
for the specific compatibility counterexamples cited above.

The previous lint and regression passes establish document structure and existing
runtime health. They do not establish soundness of this new design. No new runtime
test can prove these unimplemented plugin requirements yet. This review therefore
records explicit counterexamples and the tests needed to close each finding.
All ten findings were open at the end of that examination. No specification or
runtime code was changed during the examination.

## Specification remediation

The developer requested remediation after this review. The revised draft adds
six runtime-contract requirements and four compatibility requirements, retaining
all 31 earlier requirements in the same complete commitment. The roadmap still
selects managed language services. No plugin runtime implementation is claimed.

The following are design walkthroughs of the original counterexamples against
the revised normative rules. They are not executed plugin acceptance tests.

| Finding | Revised contract | Counterexample and corrected case |
|---|---|---|
| F01 | PCOMP-001, manifest precedence | Root skill/MCP components survive the overlay; an inline OpenAI object replaces only OpenAI settings. Canonical-file deduplication prevents repeated components. |
| F02 | PRUN-001 | Both gate/rewriter orders judge the frozen final path. A stale combined-handler allow needs a pure decision endpoint or remains held. A permitted final candidate executes once; effectful handlers are not replayed to obtain a fresh verdict. |
| F03 | PRUN-002 | A delayed result for A cannot release B after a file, absent path, directory, permission or external precondition changes. An unchanged candidate retains a valid delayed decision. External atomicity requires a real transaction rather than a precheck claim. |
| F04 | PCOMP-002 | Worktree stdout paths, replacement watch lists and Codex post-compaction stops have distinct decoding and transitions. The event/type matrix defines concurrency, special effects, ignored source fields, failures and deliberate host-policy differences. |
| F05 | PCOMP-003 | Backend compaction waits at the actual callback/core barrier. A host-only event cannot satisfy the requirement. Deny/allow and relay-loss behavior require real backend qualification; the deliverable includes the managed Codex integration needed to abort on relay loss. |
| F06 | PRUN-003 | A broken ConfigChange gate cannot intercept developer quarantine. Quarantine stops admissions and preserves holds; agent-origin requests fail, and removing required managed policy still needs its authority. |
| F07 | PRUN-004 | A v1 task retains its code/state tuple while v2 migrates a staged copy. Crashes recover the durable activation boundary. Rollback retains newer state and exposes divergence rather than silently merging or discarding it. |
| F08 | PRUN-005 | Current check receipts, review findings, todo state, allocations and developer acceptance gate real coding completion. Changing a fixture marker cannot discharge them. Cairn results map to specific actions and execution errors remain held. |
| F09 | PCOMP-004 | The committed v1 inventory fixes fields, 34 events, five handler types, test dimensions, immutable source digests and five licensed real packages. Deleting an inventoried handler must fail coverage and behavior tests. The Supabase app has an explicit service/account binding and required authenticated read-only smoke case. |
| F10 | PRUN-006 | Line-only overflow stops the producer, retains its prefix and requires an acknowledged-gap restart. Cursor sources resume from durable intake with deduplication. A crash restores recovery state without silently acknowledging loss or repeating admitted work. |

The contracts are in [plugin runtime behavior](../spec/plugin-runtime-contract.md)
and [plugin compatibility](../spec/plugin-compatibility.md). The
[frozen inventory](../spec/compatibility/plugin-profile-v1.json) records required
future test results as not run. Source digests and fixture provenance were checked
during specification work; no authenticated connector or backend barrier test ran.

The specification gaps identified here are closed. Runtime conformance remains
the full draft commitment's delivery obligation, including proprietary-backend
qualification and real connector access. A demonstrated external constraint must
return for a developer decision; it cannot become an implementation-local exclusion.
