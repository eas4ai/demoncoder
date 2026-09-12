# Non-tool lifecycle ownership and native dispatch

Status: the bounded native foundation passed specification and quality review,
including the asynchronous completion repair.

This stage adds typed non-tool operations and native UserPromptSubmit/Stop
dispatch within existing parent and child work. It does not complete lifecycle
compatibility or the skills/plugins/hooks commitment.

## Behavior and ownership

The [typed operation decision](../decisions/bind-non-tool-hooks-to-typed-lifecycle-operations.md)
keeps admission, outcomes, once consumption and asynchronous ownership in the
existing operation store. Non-tool hooks cannot borrow tool admission or invent
a ToolCall. Populated legacy tool keys retain their serialized form; the Rust
API now uses optional tool fields and an optional invocation candidate.

Production-path tests establish these bounded behaviors:

- A denied submission never reaches prompt injection or a model request. Its
  original text and refusal remain inspectable.
- Stop runs before workflow completion. Corrections use the original owner's
  allocation, deadline and correction limit; they cannot accept work.
- Repeated objections terminate. Cancel and Shutdown interrupt pending hooks
  without starting another correction.
- Developer steering is gated as a submission. Plugin corrections and observer
  context retain their origin and cannot invoke developer controls.
- Exact event, source, role, typed subject and reservation identity confer
  runner authority. Uncertain effects remain held across restart.
- Command, prompt, agent, HTTP and MCP runners have actual native Submit cases,
  including refusal before model or service I/O when ownership is missing.

An initial failing regression demonstrated cross-event reservation confusion.
Other failing controls exposed typed-subject and snapshot-policy gaps before
their repairs. Preparation failures now retain their actual reason; conservative
recovery does not falsely settle effects.

## Child ownership repair

Specification review found that the first implementation accepted only the
parent worker phase. The actual Manager uses an agent-qualified phase in the
same runtime. The [child binding decision](../decisions/separate-immutable-hook-declaration-roles-from-exact-child-execution-ownership.md)
keeps the declaration role immutable while binding execution to the exact
running assignment, identity, worktree and allocation relationship. Existing
child correction rules apply; an unsupervised child cannot borrow the parent's
correction allowance. Missing legacy child proof confers no child authority.

A full Manager launch test also exposed lifecycle plans being discarded when
constructing the restricted child policy. The repair retains those plans while
preserving confinement. Seven focused Manager tests passed, including actual
local model requests, denied and permitted submissions, bounded Stop correction,
cancellation/deadline priority, changed-owner and sibling denial, and an escaped
write refusal.

An inherited parent MCP service is rejected before TCP I/O. This proves
confinement, not child MCP support: explicit child service provisioning still
needs to reconcile immutable declaration roles with exact service-owner roles.
That integration remains required within the complete commitment.

## Cleanup oracle repair

The [cleanup decision](../decisions/verify-descendant-termination-by-identity-and-activity-within-the-existing-grace.md)
preserves direct-owner reap and the existing cancellation grace. Descendant
checks use process identity, state and observed activity. No additional grace
starts after the turn returns.

The original delayed-response failure did not record its adapter or process
state, so its historical cause remains unknown. Two subsequent test-helper
revisions made incorrect blanket assumptions about backend ownership. Actual
source inspection established that the existing fixture uses supervised Claude
and direct Codex: Claude selects supervision for its post-tool hooks; Codex
supervises this path only when a snapshot is configured. The final helper checks
those original paths without changing how the backends launch.

The final controls demonstrate:

- Killing only the supervisor leaves a known descendant writing after two
  seconds; the oracle rejects it and the fixture explicitly cleans it up.
- Normal production group cleanup stops activity and reaps its direct owner.
- Live, zombie and replaced identities remain distinct. Permission, parse and
  unexpected process-group errors still fail.
- A status file opened while a process lives can return ESRCH when read after
  the process is reaped. Actual Rust and Python controls reproduced this.
  Only NotFound/ESRCH from that process-status read mean process absence.
- Reap alone does not establish timely observation. A late observation fails
  the original absolute deadline, applied to both adapter paths.

Explicit cancellation uses the observed Cancel time plus two seconds. Hidden
correction timing uses the original enclosing integration deadline; the
lower-level real process control separately measures signal-to-stop timing.
The review does not claim that hidden cleanup start was observed, that the
historical failure was harmless, or that production cleanup code was repaired.

## Verification

The pre-Q1 190-file Rust aggregate is
`2343cc7eb76fa8d775e288e953383ba37efa552ce41cb3369a50f729ac550447`.
The parent and specification reviewer independently reproduced it. The recipe
sorts relative Path objects under src and tests, then hashes each path, NUL,
file bytes and NUL.

| Check | Observed result |
| --- | --- |
| Broad locked all-target run with no-fail-fast | 769 passed, one test-helper failure, 16 ignored; 43 targets |
| Final cleanup controls | 7 passed |
| Final affected external correction group | 20 passed |
| Final formatting and locked all-target Clippy | Passed |
| Ripwire edit check and diff check | Exit zero |
| Ripwire quality delta and test gate | Exits 2 and 4; warnings examined in quality review, not passing checks |

The broad candidate was
`800053987e3563438aa7c002dae5d0f36e5d8a2605da20da9dbed468cbe666c8`.
Its sole failure was the process-status ESRCH race in the helper. Exactly three
test files changed afterward: backend_termination.rs, external_correction.rs
and owned_process.rs. Per-file manifests confirm production source is identical.
The affected targets then passed on the final candidate. The broad command was
not rerun on that later test-only delta and is not reported as a successful
command.

Evidence is retained under /home/shawn/demoncoder-check-tmp/:

- non-tool-remediation-final-verification.json records commands, exits and hashes.
- non-tool-remediation-full-rust-mapped.log retains the broad result.
- non-tool-proc-stat-race-red.log and non-tool-proc-stat-esrch-control.json
  retain the actual read-after-reap failure.
- non-tool-proc-stat-controls-green.log and
  non-tool-remediation-external-final-oracle.log retain the final passing checks.
- non-tool-runtime-spec-review.md preserves initial findings and final approval.
- non-tool-remediation-static-analysis.txt and the final static logs retain
  warnings, including real complexity growth in the turn loop and dispatcher.

Specification review approved this exact candidate with S1/S2 resolved. Quality
review found that shared observer completion and delivery still map Submit/Stop
to PostToolUseFailure. Valid event-specific context can therefore be withheld,
and once outcomes can be wrong. Both paths require exact typed event conversion
and successful asynchronous completion/delivery tests, beyond pending-hook
cancellation coverage. That earlier candidate was not approved for commit.
The regression in non-tool-q1-completion-behavior-red.log reproduced valid
UserPromptSubmit completion being recorded as Failed instead of Succeeded.
The earlier aggregate and table above describe the candidate before that repair.
Quality review independently ran the focused non-tool library selection (16
passed) and actual Manager selection (seven passed). These selections overlap.
It found no further ownership blocker. Duplicated correction draining and the
large dispatch function remain nonblocking maintenance concerns; static
name-based false positives do not dismiss those concrete concerns. The detailed
findings are retained in non-tool-runtime-quality-review.md.

## Asynchronous completion repair

Both settlement and context reconstruction now use the same exact HookEvent
conversion. Unknown retained names return an error instead of selecting a
different event. Existing owner validation and cancellation remain in place.

The final regression runs 16 cases through native parent and actual Manager
child execution. Submit and Stop responses deliver attributed context to the
model exactly once and record successful once consumption. Wrong-event and
unknown-event responses fail; a corrupted retained event cannot deliver context.
Wrong identities, the parent and a real sibling cannot reserve child context.
Plugin-supplied command text does not accept the task.

The repaired 190-file Rust aggregate is
`5a4840d379716bb5dd95684695ef511064cbec8f2f21111794fdd976a6f6a8a0`,
independently reproduced by the parent. Only hook_types.rs, observer settlement,
observer delivery and the Manager non-tool test file changed since the earlier
quality review.

| Q1 check | Observed result |
| --- | --- |
| Full locked library | 277 passed |
| Once integration | 19 passed |
| Post-tool observer selection | Three passed |
| Final 16-case completion regression | One test passed |
| Final formatting and locked all-target Clippy | Passed |
| Ripwire edit/diff checks | Exit zero |
| Ripwire quality delta / test gate | Exits 2 / 4; warnings retained |

Only the regression harness changed after the full library and integration
runs; its affected test passed again after a small readability split and borrow
cleanup. The full all-target command was not repeated. Exact commands, logs,
exits and chronology are in non-tool-q1-verification.json. The final static
report retains 222 findings and 78 gating flags; these are not passing checks.
Follow-up specification and quality reviews approved this exact candidate.
Quality independently reran the 16-case async table and exhaustive event-name
conversion test; both passed. Q1 is resolved. The correction-drain and dispatcher
maintenance concerns remain nonblocking; no further required finding remains
for this bounded stage.

The final self-audit checked scope, existing ownership and allowance contracts,
failure/recovery behavior, retained evidence and documentation against the
production rules. It found no additional required repair for this stage.
This approval does not establish complete lifecycle compatibility or release
readiness for the overall commitment.

## Remaining commitment work

The [durable native turn decision](../decisions/give-each-native-turn-a-durable-identity-before-hook-selection.md)
was subsequently built and reviewed in
[the native turn stage](plugin-native-turn-identity.md). It supplies one real
native turn identity across Submit, Stop, internal corrections and Stop-only
configurations, with native source framing. A package dialect does not select a
backend. Actual external relay delivery remains open.

The approved [Claude](plugin-non-tool-source.md) and
[Codex](plugin-codex-non-tool-source.md) source qualifications establish actual
source facts and behavior. Authenticated host relay integration, host accounting
and cancellation still need production delivery.

Source translation, child MCP provisioning, SessionStart/End, StopFailure,
Interrupt, the remaining event matrix, component activation, management workflows
and complete installed/live conformance remain required. The plan retains one
active item: complete lifecycle dispatch.
