# Second adversarial review of the plugin specification

Date: 2026-09-09
Reviewed commit: 037ae76
Verdict: Three unresolved specification findings; revise before implementation planning.

The first remediation improves admission, state isolation, quarantine and delivery
recovery. The earlier statement that all ten findings were closed was too broad:
the compatibility inventory and dispatch contract still have concrete gaps.
The recovery design also conflicts with generation pinning.

This review does not reduce the complete deliverable or recommend an MVP.
It examines the draft, not an implemented plugin runtime. The traces below are
design counterexamples, not executed plugin acceptance tests. No specification
or runtime code changed during this review.

## R01 — P1: The frozen wire inventory loses nesting and union branches

Location: [profile inventory](../spec/compatibility/plugin-profile-v1.json),
`claude_wire.declarations`, especially BaseHookInput at line 588 and
PermissionRequestHookSpecificOutput at line 1422;
[PCOMP-002/004](../spec/plugin-compatibility.md).

The inventory is valid JSON but does not preserve the SDK's type structure.
BaseHookInput records optional `effort` with the incomplete type string `{`, then
lists required `level` alongside the outer input fields. The pinned SDK instead
places `level` inside optional `effort`. PermissionRequestHookSpecificOutput
flattens both decision variants into one field list: `behavior` appears twice,
once as allow and once as deny, with no branch or nesting identity. HookInput
has empty fields and no recorded union members.

Concrete consequence: a coverage checker can count tests for flat `level` and
both `behavior` entries while never testing `effort.level` or validating the
actual nested permission decision. A validator interpreting the listed fields
literally can reject a valid lifecycle input with no effort object. The promised
per-field deletion check does not prove correct wire coverage with these records.

Inspected the exact BaseHookInput and PermissionRequestHookSpecificOutput
declarations in the pinned Claude SDK 0.3.267 artifact. A read-only assertion
script confirmed the incomplete type, flattened required field, duplicate
unscoped branches and empty union in the committed inventory.

Required correction: preserve a structured type graph or schema, including
nesting, inherited members, discriminated unions, requiredness and references.
Give coverage entries stable paths and branch identities. Validate the generated
inventory against the pinned source rather than checking only JSON syntax and
event counts. Distinguish referenced SDK settings types from plugin features
that need executable behavior; the current closure also includes hundreds of
flattened general Settings fields.

Acceptance attack: accept a valid input without `effort`, accept nested
`effort.level`, never treat a flat level as that nested field, and distinguish allow/deny
permission branches with their permitted fields. Removing the nested-field or
branch validator must fail conformance without editing the inventory.

## R02 — P1: The event/type matrix still leaves model-hook behavior undefined

Location: [handler and event rules](../spec/plugin-compatibility.md#profile-and-wire-rules),
especially the generic prompt/agent result rows and the event table;
profile `required_handler_types` and `claude.hookModel`.

The document lists handler types separately from events and says to compose
every valid pair, but never fixes which pairs are valid. It also omits several
event-specific model-result rules. These are behavioral differences, not merely
optional JSON fields.

The [Claude hook reference](https://code.claude.com/docs/en/hooks#prompt-based-hooks)
restricts SessionStart and Setup to command/MCP handlers. A prompt hook on
PermissionRequest can run, but `ok:false` has no denial effect. On Stop, a prompt
result with `impossible:true` permits termination; agent hooks do not support
that field. Prompt and agent continuation behavior also differs by event.

The frozen inventory contains `continueOnBlock` only inside the flattened
Settings declaration and does not contain `impossible`. The host can deliberately
keep a required gate held, but it must state that translation explicitly and
distinguish a stopped task with an unmet gate from approved completion. The
current generic Boolean/verdict rule leaves implementations to choose.

Concrete consequence: two implementations can both claim every event and all
five handler types while disagreeing on whether a valid prompt response stops
work, continues correction or has no effect. Counting types and events cannot
detect the missing pair/result semantics. This is a remaining part of F04/F09.

Required correction: freeze dialect × event × handler-type applicability and
result transitions, including ignored source results and every deliberate host
difference. Include the model-response schema separately from SDK callback JSON.
Specify how termination, unmet policy and completion differ.

Acceptance attack: exercise the three cases above plus PreToolUse/PostToolUse
with both values of `continueOnBlock`; compare actual tool execution, follow-up
turns, stopped state and completion state. Deleting a pair-specific translation
must fail the frozen coverage check.

## R03 — P1: Repair has no permitted transition for an already pinned task

Location: [EXT-004](../spec/extensions.md), lines 45–53;
[configuration recovery](../spec/plugin-runtime-contract.md#configuration-recovery),
especially lines 143–145;
[state pinning](../spec/plugin-runtime-contract.md#mutable-state-and-activation).

EXT-004 pins both the generation and policy to each admitted task and child.
Its falsifier explicitly rejects mixing old instructions with a new handler.
The new recovery contract says that replacing quarantined code creates fresh
decisions against affected work. It supplies no exception or task transition
that makes those decisions usable by a task pinned to the broken generation.

Counterexample:

1. Task T starts with generation A and its required gate.
2. That gate fails; the developer quarantines A, leaving T held.
3. The authorized developer installs repaired generation B.
4. T either retains A and cannot pass, or runs B's handler against its existing
   instructions and violates EXT-004. Automatically approving T is prohibited.

Starting an entirely new task might be a valid recovery choice, but the contract
does not define transfer of completed effects, evidence, children, state pins or
the remaining cumulative allowance. The new explicit-attempt rule concerns
candidate revalidation; it does not authorize generation replacement.

Required correction: define one explicit developer-authorized recovery
transition. Either keep T held and create a linked replacement task with retained
effects and remaining allowance, or specify a generation-rebinding transaction
with the required instruction, child, state and evidence invalidation. Amend
the pinning rule to name that exception. Ordinary reload must remain distinct.

Acceptance attack: hold a task after one completed mutation, quarantine its
failing gate, install the repair and recover through the developer control.
The mutation must not repeat, the allocation must not reset, old approvals must
not survive, and the resumed/replacement task must have one coherent generation.

## Checks and limits

Read the revised runtime, compatibility, component, extension and lifecycle
contracts and their proposal and commitment. Compared the frozen JSON with its
pinned SDK declarations. Checked current official hook documentation for the
specific event/type/result differences above. Manifest overlay precedence and
Claude custom-path composition were also rechecked; neither produced a new
finding. The explicit backend qualification requirement appropriately avoids
claiming that a relay is already proven to fail closed.

This review changes only review records. The existing LSP regression evidence
does not establish plugin compatibility or close these specification findings.
