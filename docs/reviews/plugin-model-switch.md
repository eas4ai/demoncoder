# Actual Creator model switches

Status: The [bounded implementation decision](../decisions/own-actual-creator-model-switches-through-the-original-host-session.md)
is implemented and independently approved. Final Rust, installed-backend and
terminal development checks pass. The qualification record below retains the
unexplained earlier compaction failure and nonpassing static diagnostics.
Complete lifecycle dispatch remains the sole task in progress; this is not
whole-commitment acceptance or committed-input Cairn evidence.

## Contract and existing boundary

PreModelSwitch gates an eligible actual model change. PostModelSwitch observes
the applied change and may contribute bounded plugin-origin context. A saved
default alone does not establish a switch. Existing assignments, captured work,
provider identity, original allowance and opaque backend context rules still apply.

The pre-change `WorkflowSession::refresh_creator` consumes an eligible captured
selection. It opens a replacement, optionally restores a compatible native
checkpoint, stops old observers, records the Creator binding, closes the previous
session, replaces the live connection and refreshes Settings ownership. It emits
ModelAssignment afterward. Close failure holds the runtime. The implementation
must distinguish a blocked switch from a partially applied or uncertain one.

The frozen compatibility inventory lists native handlers of all five types and
Claude command, HTTP and MCP handlers for these events. Codex has no source event.
Claude's input requires context/cache/pricing values; the host cannot substitute
invented zeros or label a host replacement as a backend callback. Native facts and
genuine backend source facts need their respective schemas and provenance.

## Installed Claude source probes

The first three probes used `/home/shawn/.local/share/claude/versions/2.1.267`, verified before
execution against SHA-256
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`.
They used disposable workspaces and homes, synthetic authentication, an isolated
environment and a local HTTP endpoint. No HTTP request occurred. Each CLI process
was terminated and waited for; no real account or saved application setting changed.

Artifacts live under `/home/shawn/demoncoder-check-tmp/`. Each probe directory
contains `inputs.json`, `sent.json`, `observed.json`, `result.json` and stderr.
Inputs retain the exact argv, synthetic environment, binary hash and pre-run
probe source hash. Probe scripts remain separately retained and unchanged.

- `model-switch-claude-source-probe-20260913T072919238066Z`: initialization and
  `set_model` succeeded. The real PreModelSwitch callback supplied all required
  input fields, including `source: sdk`, cache TTL and catalog pricing. This
  first probe stopped at the control response, so it cannot establish whether
  a later PostModelSwitch callback exists.
- `model-switch-claude-source-sequence-20260913T073035104944Z`: a denied change
  returned the actual hook-blocked error. The next request still reported the
  old Sonnet model. Allowing that request emitted PreModelSwitch and
  PostModelSwitch; the following reverse switch reported Opus as its original
  model, then emitted both events. This demonstrates the source veto and actual
  source state changes for these SDK controls.
- `model-switch-claude-source-alias-20260913T074103631923Z`: the denied and then
  allowed request `opus` resolved to `claude-opus-5` in the real callback. Resetting
  the model with JSON null resolved to `claude-sonnet-5`; both allowed changes
  emitted real post callbacks. These are observations of this pinned executable,
  not assumptions about another release's aliases or defaults. Requested choice
  and resolved model must remain distinct, and Pre/Post must agree on the same
  resolved candidate. No model request was made.

PostModelSwitch arrived after the successful `set_model` control response. An
adapter must continue driving that callback; stopping its receiver at the first
success would miss a real observation. Cache and token values in these probes
describe an empty backend conversation. They do not establish nonempty-context
or live-provider values, and the control probe is not an application gate test.

### A callback relay error does not veto the installed source

An additional source-only probe used the same pinned binary and isolation:
`model-switch-claude-source-relay-error-20260913T083250198370Z`.
Its unchanged script is `model-switch-claude-source-relay-error-probe.py`.
The probe answered the real Pre callback with a correctly correlated protocol
error containing a synthetic relay-failure message. Claude returned `set_model`
success and emitted genuine PostModelSwitch. The next reverse Pre callback named
Opus as its original model, confirming that the first switch had applied.
An explicit hook `permissionDecision: deny` afterward still blocked a switch.

Thus a callback protocol error cannot be treated as a source veto. When possible,
the host must return a valid explicit denial for a refused or failed Pre gate.
If delivery is lost, the host must retain uncertainty and prevent the dependent
prompt from executing; it cannot claim the SDK kept the old model. Application
relay-loss qualification is still required. This probe made no HTTP request and
its process was terminated and waited for. `parent-audit.json` binds the unchanged
probe source and raw records; cleanup returned 143. The parent's first audit
incorrectly expected -15 and failed before writing; the corrected audit does not
change any source observation.

The immutable inventory
`lifecycle-model-switch-source-observations-20260913T084100489187Z.json`
binds all four source probes and their 25 retained inputs with SHA-256 hashes.
Its own SHA-256 is
`e631e47c86cf9cf061d38b71a2341d4d6fc5f4e462a9ee33574aef32c1c81ad5`.
This inventory audits existing observations; it runs no new backend behavior and
does not freeze or qualify the application candidate.

### A proven unchanged source model emits no switch callbacks

A fifth source probe,
`model-switch-claude-source-noop-20260913T084300000834Z`,
set the initial concrete Sonnet model to itself, switched to the `opus` alias,
then selected the concrete `claude-opus-5` model that alias had just resolved to.
All three controls succeeded. Only the middle control emitted Pre/Post; the two
unchanged-model controls emitted neither during their responses and the following
0.5-second collection windows. The probe made no HTTP request and waited for
process cleanup. Its separate `parent-audit.json` binds the six retained inputs;
it is not part of the earlier four-probe inventory.

The actual initialization response supplies model entries with both `value` and
`resolvedModel`. That gives the adapter a source-owned resolution map for this
initialized backend, alongside concrete models from genuine callbacks. No-op
handling must establish unchanged identity from trustworthy captured facts.
It must not classify every success without Pre as a harmless no-op, fabricate
callbacks, hardcode this probe's alias catalog or let a missing required gate
release a real change. The application's no-op path remains to be qualified.

## Planned application checks and review attacks

These are verification obligations, not recorded passes.

| Boundary | Observable failure control and corrected behavior |
| --- | --- |
| Eligibility and identity | Saving a default during a held request must not alter that request. New eligible work uses its captured assignment; label-only changes and explicit overrides must not emit a false switch. |
| Required policy | A denying real runner must keep old provider state and prevent new-provider requests. The dependent submitted prompt must not fall back to an additional old-provider request; retain its text and visible refusal for explicit repair/resubmission. A later allow must reach the new provider. An unavailable required Claude source must hold rather than disappear from dispatch. |
| Original authority | Model/service hooks spend the original host grant and deadline. A selected configuration with larger limits, an unrelated active task or a replacement account cannot renew or lend funding. Repeat A→B→C using the same original host authority: the second switch must remain eligible without resetting counters or weakening ordinary native identity checks. |
| Final candidate | Hold a gate while changing a relevant workspace input or invalidating its owner. The stale result must not release the switch, rerun effects or silently choose a later Settings default. |
| Source correlation | Native and genuine Claude callbacks belong to one switch. Duplicate, wrong-session, wrong-model, late and forged callbacks must not create a second execution or release another occurrence. |
| Actual application | Inject old-provider close failure, durable binding failure and Settings-control activation failure. Recorded identity, live provider and applied/uncertain status must agree with the stage actually reached. |
| Post observation | Delay Post until after SDK control success. Observe the actual applied model once; retain it across observer rejection, malformed output or cancellation. Ordinary observer failure cannot become a veto. |
| Cancellation and recovery | Drive actual session Cancel/Shutdown commands while Pre is held, and cancel before the gate, at teardown and after source application. Join owned handlers; retain uncertainty and completed effects. Restart must not replay the switch or its side effects. |
| Existing boundaries | Repeat the original Settings ownership, native lifetime and MCP reuse cases. Exercise actual SessionEnd dispatch and settlement after repeated native switches and a native/external owner change. A new transition token must not weaken identity checks or transfer authority. |
| Terminal behavior | Preserve held work, prompt draft, history, model labels and bounded responsiveness. Actual old/new transport requests supply identity evidence; rendered labels alone do not. |

## First application failure control

The paused-close Settings ownership test reproduced the existing ordering problem:
while the old provider was still closing, the durable Creator record already named
the replacement. The actual failing command is retained under
`lifecycle-model-switch-red-close-before-bind-corrected-20260913T074040119957Z`
in the same scratch directory. It ran one test and failed the explicit old-identity
assertion. An earlier test edit matched a different assertion and passed; that
earlier result is not the violating control.

The initial correction moves durable binding after successful close and revokes
pending Settings-control authority before teardown. With identity still old at
that point, the stale Settings proposal reports an invalidated policy/host owner
instead of a prematurely changed identity. The corrected test also checks that
the invalidated proposal cannot proceed into its later MCP handler.

`lifecycle-model-switch-green-close-settings-invalidation-2-20260913T074245884957Z`
passed exactly one test. Other Rust targets in that filtered invocation selected
zero tests; this is not a full regression. The parent read the actual RED/GREEN
logs and exit metadata. The complete switch operation, source bridge and later
failure/recovery checks remain under implementation and review.

## Initial native ownership and refusal checks

The focused checks below ran during implementation, before the complete candidate
was frozen. Each selected one library test. They establish those observed paths,
not complete runner, backend or recovery qualification.

| Artifact prefix, under the same scratch directory | Observed result |
| --- | --- |
| `lifecycle-model-switch-green-nontool-owned-2-20260913T074632590943Z` | The registered model-switch events use the owned non-tool path; Post remains observation-only. |
| `lifecycle-model-switch-green-runtime-owner-2-20260913T075200971039Z` | The staged durable owner inherits the original session allowance and applies the binding once. |
| `lifecycle-model-switch-green-runtime-original-funding-2-20260913T075745609434Z` | Pre and Post share the exact switch owner and route receipts back to the original SessionHooks funding. |
| `lifecycle-model-switch-green-native-deny-visible-retained-2-20260913T081113895167Z` | A real native Pre refusal leaves the old identity, retains the submitted text and visible refusal, and sends no additional old/new provider request. |
| `lifecycle-model-switch-green-native-deny-allow-receipts-2-20260913T081301303823Z` | A later explicit submission applies the selected model. The prior refused submission stays unexecuted; denied Pre, allowed Pre and Post each remain attributed once. |

The first deny/allow run,
`lifecycle-model-switch-green-native-deny-allow-receipts-20260913T081200518685Z`,
failed after its 40-second turn bound. Its filename is only the requested run
label. The implementation held the Pre validation mutex while Post tried to inspect;
the correction releases that boundary immediately after durable/live application.
The failed log and its exit 101 remain intact alongside the corrected passing run.

The parent read these raw results and metadata. For the original-funding,
occurrence and initial deny/allow failure records, it also verified the captured
log hash. Intermediate dead-code warnings are visible in several runs while the
source bridge is incomplete; a final warnings-denied check remains required.
The MCP binding seam compiles in
`lifecycle-model-switch-compile-model-switch-service-binding-20260913T081423732804Z`,
but that check does not exercise actual Pre/Post service reuse or exhaustion.

## Initial installed bridge and repeated-switch checks

`lifecycle-model-switch-installed-claude-source-deny-allow-green-20260913T083036793022Z`
explicitly ran the installed-Claude library test with `--ignored`. It passed one
test in 3.17 seconds using the pinned 2.1.267 executable. The application refused
the first real SDK switch without a new provider request, then allowed a fresh
switch, observed the genuine Post callback and sent the next request using Opus.
This uses a local controlled provider endpoint; it is not live-provider evidence.

The parent also identified a repeated-switch ownership problem in the draft:
ordinary native lifetime validation requires the current identity to equal the
original one. Applying that rule directly would reject B→C after a valid A→B.
The implementation now validates the ModelSwitch chain from the same live original
lifetime through completed, unheld, applied switches. Ordinary native lifetime
validation stays strict.
`lifecycle-model-switch-repeated-switch-original-lifetime-20260913T083146622885Z`
passed one runtime test with the original allowance start, deadline, model and
tool counters unchanged across two switches. Actual MCP service-capacity reuse
is a separate check still pending at this stage.

The parent read both raw logs, checked their log hashes against metadata and
confirmed that neither result came from zero matched tests. These are development
results; the final candidate, complete failure cases and independent reviews
remain pending.

## MCP capacity and original session shutdown

`lifecycle-model-switch-mcp-consecutive-green-5-20260913T084311537766Z`
passed one actual library test. A→B→C reused one MCP initialization and consumed
exactly three tool calls under `max_calls=3`. The fourth Post observer attempt
reported exhaustion while C remained applied. The original session allowance did
not restart. This closes the earlier consecutive-switch MCP capacity question for
that exercised service path.

The shutdown attack reproduced a missing original SessionEnd handler after A→B.
`lifecycle-model-switch-native-session-end-red-20260913T085133573420Z`
failed its expected handler-count assertion, observing two calls instead of three.
The correction retains the closed original native owner solely for its exact end
policy. SessionEnd may validate the current identity through completed, unheld
ModelSwitch records from that original lifetime; general `validate_live` remains
unchanged. The bounded correction fits the existing implementation decision.
`lifecycle-model-switch-native-session-end-green-2-20260913T085340966567Z`
passed the same one-test filter. The native-to-external shutdown case and complete
regressions remain pending at this stage.

The parent read these raw logs and exit metadata and checked their log hashes.
No final application freeze or complete qualification is implied by these focused
development checks.

The subsequent focused checks each passed one explicitly selected library test:

- `lifecycle-model-switch-native-to-external-session-end-3-20260913T085636803379Z`
  ran with `--ignored` and the pinned Claude executable. It retained exactly the
  original native SessionEnd owner across the change to the external backend.
- `lifecycle-model-switch-installed-claude-runner-error-2-20260913T085757541351Z`
  ran the installed case with `--ignored`. A side-effect-free runner failure
  reached genuine Pre, produced an acknowledged explicit denial and preserved
  the old identity with no new provider request.
- `lifecycle-model-switch-cancel-held-pre-green-20260913T090110604655Z`
  drove the actual Cancel command while Pre was held. The turn cancelled,
  identity and provider-request count stayed unchanged, and the runtime retained
  a conservative recovery hold without replay.
- `lifecycle-model-switch-installed-claude-relay-loss-green-20260913T090123211108Z`
  ran the installed relay-loss case with `--ignored` and retained the hold without
  provider replay.

The parent read all four raw logs and their selected commands, confirmed actual
one-test passes and verified their log hashes. These development results use
local provider fixtures; the complete candidate and independent reviews remain
pending.


## Expanded owner, runner and failure qualification

The following development results extend the earlier checks. Their artifact
prefixes are under the same scratch directory. The parent read the raw results
and checked each log hash against its recorded metadata. Each owner/runner/failure
row selected one actual test; none is a full regression or a frozen-candidate result.

| Artifact prefix | Observed result |
| --- | --- |
| `lifecycle-model-switch-native-five-runner-matrix-2-20260913T090420236942Z` | Native Pre ran command, HTTP, MCP, prompt and agent with the original session grant. |
| `lifecycle-model-switch-model-switch-anthropic-native-owner-20260913T091428263866Z` | Anthropic API used the native host switch owner. Together with the existing OpenAI checks, both native API owners are exercised. |
| `lifecycle-model-switch-model-switch-codex-native-owner-20260913T091443874561Z` | Pinned installed Codex used host-native Pre/Post without a fabricated source callback. The test was explicitly selected with `--ignored`. |
| `lifecycle-model-switch-model-switch-claude-post-context-final-20260913T092445178456Z` | Pinned installed Claude exercised genuine denial, fresh allow and Post through command, HTTP and MCP. It retained one MCP initialization, validated genuine cache/pricing input, and placed Post context in the actual next controlled provider request. A proven canonical no-op emitted no extra callback. The test was explicitly selected with `--ignored`. |
| `lifecycle-model-switch-model-switch-stale-inspected-green-20260913T092127210959Z` | Changed inspected input refused the transition before teardown, preserving old identity and provider without recovery or replay. |
| `lifecycle-model-switch-model-switch-persistence-failure-green-20260913T092245192272Z` | Application persistence failure latched execution failure and retained a readable old identity at Teardown on disk. Actual reopen is a separate pending check. |
| `lifecycle-model-switch-model-switch-post-observer-visible-green-20260913T091933939650Z` | An ordinary Post error stayed visible; the applied identity and provider request remained, with no recovery hold. |

`lifecycle-model-switch-model-switch-grouped-lib-final-20260913T092415625311Z`
passed 13 tests, ignored four installed-only tests and filtered out 481 tests.
`lifecycle-model-switch-model-switch-clippy-green-20260913T092358501308Z`
passed `cargo clippy --locked --lib -- -D warnings`. These results precede the
remaining changes; the word `final` in a requested label does not freeze a candidate.

Failed fixture controls remain intact. The first native five-runner attempt
underflowed a test counter. The initial stale-input assertion expected the wrong
error phrase; the diagnostic rerun showed the actual refusal was already
`lifecycle inspected inputs changed before continuation`. The first persistence
test tried to acquire a second store lock while the session was still open and
failed with `session is already open`. Those failures are not evidence that the
production gate allowed a stale switch or that the persisted snapshot was corrupt.

The implementer released the initial candidate with four exact attacks deferred:
backend close error, Settings activation failure after application, reopen from
the retained Teardown snapshot without replay, and unrelated held/queued work
interleaving. The parent returned these explicit decision obligations to the
implementer before formal SPEC review. They remain unqualified here.

Ripwire also returned nonpassing results: quality-delta exited 2 and test-gate
exited 4. Independent review must assess the reported complexity, size, clone and
test-mapping findings alongside actual behavior coverage. No suppression or
baseline change has converted those reports to passes. Full regression, independent
SPEC and QUALITY, final source integrity and the production self-audit remain open.


## Direct teardown, activation, reopen and interleaving checks

The parent read and hash-verified each following raw result; every row selected
one actual library test and passed. These are development checks before source freeze.

| Artifact prefix | Observed result |
| --- | --- |
| `lifecycle-model-switch-model-switch-close-failure-red-20260913T093425126723Z` | Injected old-provider close failure retained Teardown, old durable identity and a recovery hold, without publication or replay. Despite its requested `red` label, this command passed; it is not a failing implementation control. |
| `lifecycle-model-switch-model-switch-settings-activation-failure-control-20260913T093446677851Z` | Settings activation failure after application preserved the new identity and held the dependent prompt. |
| `lifecycle-model-switch-model-switch-reopen-teardown-control-20260913T093534539724Z` | Actual reopen from Teardown marked recovery and did not replay the switch. |
| `lifecycle-model-switch-model-switch-interleaved-assignment-green-20260913T093747583371Z` | While the gate held captured B, saving C did not replace that captured choice. B applied, the concurrent queued prompt was refused, and a fresh prompt then selected C. Original allowance counters and deadline did not renew. |

The interleaving fixture initially placed its Settings file inside the gate's
inspected workspace. Saving C therefore caused the intended stale-input refusal,
reported in the retained control and diagnostic logs. The corrected unrelated-input
case keeps that file outside the inspected workspace. It does not weaken read-set
validation or turn the earlier refusal into a claimed production defect.

`lifecycle-model-switch-model-switch-four-gap-grouped-20260913T093819418488Z`
passed 14 tests with four installed-only tests ignored and 484 filtered out.
Three of the four new cases have names outside that filter; their individual
passes above supply their evidence.
`lifecycle-model-switch-model-switch-four-gap-clippy-20260913T093829910578Z`
passed library Clippy with warnings denied.

## Parent finding: stale callbacks across repeated model cycles

The dedicated Claude callback owner generated registration IDs once per adapter
and reset its sequence for each switch. It checked the current session and models,
but retained no earlier request ID or tool-use envelope ID. The parent identified
that a delayed A-to-B callback could match a later A-to-B after A-to-B-to-A.
The implementer confirmed no additional transport/runtime replay check disproved
that path. Generic within-switch duplicate checks do not establish this boundary.

The repair must reject reused callback request and envelope IDs before altering
the new candidate or dispatching handlers. Retain bounded protection for the live
registered owner across switches, and hold visibly if its bound is exhausted.
The specific failure control, corrected repeated-cycle behavior and malformed,
wrong-session/model and forged-registration controls remain pending. This repairs
the existing exact-callback contract; it does not add a new lifecycle feature.


## Callback repair qualification and source release

The repair keeps request and envelope IDs for the live registered callback owner
across per-switch resets, rejects duplicates before dispatch, and holds on overflow
without eviction. Controlled malformed/correlation and replay-bound tests passed
in `lifecycle-model-switch-model-switch-callback-correlation-green-20260913T094127524176Z`.
The handle-level old Pre/Post test passed in
`lifecycle-model-switch-model-switch-callback-handle-green-2-20260913T094258729768Z`.
Pinned installed Claude then passed genuine callbacks, all three source runner types,
alias/no-op and Post context in
`lifecycle-model-switch-model-switch-claude-replay-guard-installed-final-20260913T094316229470Z`.
Library Clippy passed in
`lifecycle-model-switch-model-switch-callback-four-gap-clippy-20260913T094326245974Z`.
The parent read the raw results and verified their log hashes.

A fresh failure demonstration disabled only the retained-ID claim, keeping the
complete handler and its test intact. Its actual mutated source and changed-source
manifest were captured before execution in
`model-switch-callback-handle-red-source-20260913T054655239731Z/manifest.json`.
`lifecycle-model-switch-model-switch-callback-handle-replay-red-valid-20260913T094703372209Z`
ran one test and failed its replay-refusal assertion. Restoring the exact original
callback module produced the one-test pass
`lifecycle-model-switch-model-switch-callback-handle-replay-green-exact-restore-20260913T094725639028Z`.
The parent verified the retained mutated hash, restored original hash, unchanged
other source/test hashes and both raw log hashes. This is a controlled deletion
attack at the dispatch boundary, not an installed-source failure demonstration.

The earlier helper-level RED used a bypass too, but its source was reconstructed
later; it is not an original candidate capture. A preliminary fresh mutation was
compile-invalid and is excluded from the behavioral claim. Neither failed record
was edited into a pass.

After exact restoration,
`lifecycle-model-switch-model-switch-grouped-lib-after-handle-replay-20260913T094750680289Z`
passed 18 tests with four installed-only tests ignored and 484 filtered out.
`lifecycle-model-switch-model-switch-fmt-after-handle-replay-20260913T094757948582Z`
passed formatting. The source implementer released ownership with only the static
assessment and independent/broad verification outstanding.

The retained static reports are `model-switch-final-quality-delta-20260913T0943.xml`
and `model-switch-final-test-gate-20260913T0943.xml`, each with its original exit file.
Quality reports 210 findings: 98 worsened existing symbols, 112 new symbols,
33 minor and 65 gating findings, with zero acknowledgments. Test-gate reports
18 changed and 1,368 impacted symbols, 53 mapped targets and 593 untested impacted
symbols. These counts differ from the earlier preliminary report because the
candidate changed. The full Rust suite covers ordinary mapped Rust targets;
additional installed Submit/Stop baselines and terminal output-limit tests are
queued. The committed-input live Oracle mechanism remains for Cairn's committed
verification; an uncommitted development run cannot refresh that evidence.


## Independent SPEC: four required repairs

The independent review of source freeze `lifecycle-model-switch-source-freeze-20260913T094948305698Z`
returned **FAIL**. The immutable report is `model-switch-spec-review-20260913T095822373329Z.md` in the scratch directory,
SHA-256 `7ca947874a769871c0e91015043a306ce79ae30962be98e731ae5254aa25ee04`. The reviewer inspected actual code and retained results and
verified all 2,833 frozen paths and nine external inputs. It ran no runtime tests.

1. Claude drops the exact final inspected-input validation token before genuine
   Pre permission delivery. Retain and validate it at the serialized release
   boundary, including original owner/funding and recovery. Known pre-effect
   release failures need explicit denial when the callback channel is available.
2. Alias-to-canonical no-op advances requested identity outside the exact switch
   chain, preventing a later genuine switch or original native SessionEnd. Keep
   effective identity provably tied to the original lifetime without fabricated
   model events, arbitrary history-based authority or general identity exceptions.
3. Claude Pre-only and Post-only configurations cannot validate the absent side's
   authenticated empty source observation against a `None` plan pin. Pin that
   observation explicitly without inventing a user declaration or permitting a
   forged nonempty plan.
4. Native final validation waits outside Cancel/Shutdown handling, and irreversible
   release does not recheck newly raised recovery/task conditions. Make this
   boundary cancellable and validate admission before teardown. After SDK permission
   or actual effect, retain known application or uncertainty; a broad recovery
   check must not prevent truthful applied-state persistence.

Required controls include genuine-source stale input at final release, available
explicit-denial delivery, source after-allow loss, no-op followed by another genuine
switch and original SessionEnd, one-sided source declarations, native final-wait
Cancel and Shutdown, and recovery raised before versus after irreversible release.
The parent recorded these findings before returning source ownership for repair.
Full regression and independent QUALITY remain unstarted pending corrected SPEC.

## Corrections submitted for repeat SPEC

The implementer released the repaired source after focused verification. Claude
retains the exact inspected-input validation through permission and SDK effect;
known pre-effect failures send explicit denial over the usable callback channel.
An authenticated alias/canonical no-op preserves the effective identity. Absent
Pre or Post declarations pin the authentic empty source observation. Native final
validation handles Cancel and Shutdown, and teardown rechecks recovery, task,
original owner, allowance and deadline. These are submitted repairs, not an
independent passing verdict.

The parent checked raw result summaries and recorded log hashes for the behavioral
RED runs at `spec1-final-validation-red-exact-20260913T100620070568Z`,
`spec1-recovery-release-red-20260913T100925270577Z`,
`spec2-alias-next-switch-red-20260913T101313365170Z`,
`spec3-pre-only-red-20260913T102046863779Z`,
`spec3-post-only-red-20260913T102103670922Z`,
`spec4-final-cancel-red-valid-20260913T102608346338Z`, and
`spec4-final-shutdown-red-valid-20260913T102636685082Z`. Each selected one test
and failed its behavioral assertion. These suffixes all follow the exact prefix
`lifecycle-model-switch-model-switch-` in the scratch directory. Pre-run changed
source manifests and exact focused GREEN paths are recorded in
`model-switch-implementation-handoff.md`, under Independent SPEC repair.

The following final development runs passed; the parent verified their raw log
hashes and nonzero counts. They share that same artifact prefix.

| Suffix | Observed result |
| --- | --- |
| `spec-repair-grouped-lib-20260913T103327430309Z` | 20 passed, five installed-only ignored; 493 filtered out. |
| `spec-repair-installed-claude-group-green-20260913T103444148194Z` | 11 installed cases passed serially, with both pinned backends available. |
| `spec-repair-native-claude-session-end-20260913T103554229503Z` | One installed native-to-Claude-to-no-op original SessionEnd case passed. |
| `spec-repair-native-final-command-group-20260913T103611002326Z` | Actual Cancel and Shutdown at final validation passed, two tests. |
| `spec-repair-native-final-recovery-20260913T103616313440Z` | Recovery raised after native Pre blocked teardown without replay, one test. |

The installed group includes stale inspected input, explicit pre-effect recovery
denial, Pre-only, Post-only, no-plan, relay loss, runner error, genuine callbacks,
and both post-effect controls. Post-effect Cancel preserves the old durable
identity under uncertainty; known SDK success followed by recovery records the
applied identity while holding further work. Neither sends the dependent prompt.
The source-alias test's diagnostic Haiku exchange produced two different genuine
Pre candidates; exact correlation refused the changed candidate. The cause is
unknown. The successful next-switch case uses catalog-known Sonnet and does not
weaken the correlation rule.

Formatting, library Clippy with warnings denied, and diff check passed at
`spec-repair-fmt-final-20260913T103622510432Z`,
`spec-repair-clippy-final-20260913T103626893204Z`, and
`spec-repair-diff-check-final-20260913T103913540749Z`. A nominal Cancel GREEN
selected zero tests and is excluded. Earlier installed runs missing required
backend environment variables and the first formatting failure remain nonpasses.

Fresh static reports `model-switch-spec-repair-final-quality-delta.xml` and
`model-switch-spec-repair-final-test-gate.xml` retain exits 2 and 4. Quality names
256 findings: 106 worsened existing symbols, 150 new, 36 minor and 70 gating,
with zero acknowledgments. Test-gate names 53 mapped targets and 595 untested
impacted symbols among 1,373 impacted symbols. These results remain unwaived.
Corrected SPEC, independent QUALITY, full regression and final integrity checks
remain required before the bounded decision can be marked built.

## Repeat SPEC: correct the deadline owner

The independent review of source freeze `lifecycle-model-switch-source-freeze-20260913T104402203455Z`
confirmed all four original repairs, but returned **FAIL** for a new P1. The
immutable report is `model-switch-spec-review-20260913T104840505451Z.md`, SHA-256
`8a09dc43b2bb1b520b18df0156981212dd230b159bf056cd04168043099c6ad1`.
The reviewer inspected source and existing evidence and verified the complete
candidate; it ran no runtime tests.

Final teardown checks the original lifetime's 30-second SessionStart observation
window. That window does not determine whether the original cumulative
SessionHooks allowance still has time. A valid older session can be refused,
while a shorter original allowance expiring after Pre can escape this check.
Resolving an active allocation does not itself check its remaining time.

Use the applicable original operation/SessionHooks deadline before irreversible
release, preserving exact original authority, budget reference, counters and
grant-free synchronous-command semantics. Do not renew the startup window or
create a new grant. Already applied SDK effects must still be recorded truthfully
when time expires or recovery is raised afterward.

Required production controls: native and installed Claude switches after only
the observation window expires; actual allowance expiry after Pre before release
with native refusal and available Claude explicit denial; grant-free command and
no-plan behavior after the observation window; ended/revoked owner refusal; and
preserved post-effect cancellation/recovery behavior. The parent recorded this
finding before returning the bounded correction to the implementer.

## Deadline repair development controls

Four production-boundary tests reproduced the deadline defect before the repair.
The exact pre-run source capture is `model-switch-deadline-red-source-20260913T105426728207Z/manifest.json`,
SHA-256 `e32de67342db15864b04f5b30916d94aac2e3285e4d08a221cb6fb00eaebf2c1`.
The parent verified all ten retained source copies against that manifest and
verified the actual test counts and raw log hashes for the following RED/GREEN
pairs. Every suffix below follows `lifecycle-model-switch-model-switch-deadline-`.

| Boundary | RED suffix, one failed test | GREEN suffix, one passed test |
| --- | --- | --- |
| Native: expired startup observation, live original allowance | `native-aged-red-20260913T105430941457Z` | `native-aged-green-20260913T105537021462Z` |
| Native: allowance expires after Pre | `native-allowance-expiry-red-20260913T105435284636Z` | `native-allowance-expiry-green-20260913T105546863678Z` |
| Installed Claude: expired startup observation, live original allowance | `claude-aged-red-20260913T105446468498Z` | `claude-aged-green-20260913T105553079549Z` |
| Installed Claude: allowance expires after Pre | `claude-allowance-expiry-red-20260913T105455052458Z` | `claude-allowance-expiry-green-20260913T105604203658Z` |

The aged-owner REDs refused a valid switch; the expiry REDs completed a switch
that should have been refused. These are actual behavioral failures. The repair
uses the original applicable allocation's remaining time at final release.
`grant-free-green-20260913T105614610582Z` passed two synchronous command/no-plan
cases after observation expiry. `ended-revoked-green-20260913T105620436659Z`
passed one original-owner refusal test. The parent checked both commands,
nonzero counts and log hashes. Installed cases used the pinned Claude executable
and controlled local peer. Independent review and final regression remain open.

After the deadline fix, installed post-effect Cancel and recovery passed again at
`lifecycle-model-switch-model-switch-deadline-post-effect-cancel-green-20260913T105655017363Z`
and `lifecycle-model-switch-model-switch-deadline-post-effect-recovery-green-20260913T105708653737Z`.
The grouped filter at `lifecycle-model-switch-model-switch-deadline-grouped-lib-20260913T105722983358Z`
passed 25 tests with six installed-only cases ignored. Final formatting, library
Clippy with warnings denied, and diff check passed at deadline suffixes
`fmt-final-20260913T105749458000Z`, `clippy-final-20260913T105755448179Z`, and
`diff-final-20260913T105806558931Z`. The parent checked commands, raw counts and
log hashes. The earlier formatting check remains an unchanged failure.

The worker released the candidate. Fresh `model-switch-deadline-final-quality-delta.xml`
and `model-switch-deadline-final-test-gate.xml` retain exits 2 and 4. Quality reports
287 findings, 106 worsened existing symbols, 181 new, 36 minor, 70 gating and
zero acknowledgments. Test-gate reports 53 mapped targets and 596 untested
impacted symbols among 1,374 impacted. These remain for independent assessment;
no suppression, baseline change or passing claim was made.

## Specification pass, full-build failure and QUALITY findings

Independent SPEC passed source freeze `lifecycle-model-switch-source-freeze-20260913T110020157677Z`.
The immutable report `model-switch-spec-review-20260913T110333084126Z.md` has
SHA-256 `409d1a6bffb4afabf0e8ff506de8cd8e1f2bbdfeb533c5c835a511ca60e1c7a6`.
All five specification findings were resolved. The reviewer verified all 2,833
frozen paths and external inputs and ran no runtime tests.

The parent then ran `cargo test --locked --all-targets --no-fail-fast` alongside
independent QUALITY. It failed during compilation: an exhaustive occurrence
match in `tests/plugin_command_runners.rs` omits PreModelSwitch/PostModelSwitch.
No test suite ran, and the final-check sequencer stopped before later checks.
The raw result `lifecycle-model-switch-all-targets-final-20260913T110420269098Z`
retains exit 101 and log SHA-256
`dd4da56aace9f3cbe8f08236ba9853fce8ace4cab3c218fa0e173ce17eb47248`.
This is an ordinary E0004 compile failure, not a compiler crash.

QUALITY returned **FAIL** with two required repairs. Its immutable report is
`model-switch-quality-review-20260913T110951350384Z.md`, SHA-256
`c81728e392af24f2f46d0368eab61f25794ec746a61c5b76e243001fa99a2979`.
The parent read the full report and recorded its findings before source repair.

1. Add explicit rejection arms for the two new events in the ordinary-turn test
   fixture. Preserve its unexpected-event assertion and exhaustive matching.
2. Publish Claude's authenticated resolved model in the applied ModelAssignment
   event. The terminal consumes that field directly, while the current event
   reports the requested alias. Preserve the requested durable connection identity.
   Extend the installed alias/no-op/next-switch test to compare the assignment
   event, genuine source model and provider request, with no applied event on denial.

The reviewer assessed all 287 static quality rows and the 53 mapped test targets.
It distinguished trait/test-harness graph gaps and ambiguous unchanged-file
attribution from actual complexity and repeated setup. The visible untested list
covers only 25 of 596 symbols; omitted symbols were not individually assessed.
Duplicate callback validation and repeated test setup/event draining are recorded
as minor maintenance notes. They do not justify a broader rewrite or changing
the original static results. The full build, installed/terminal checks, corrected
review and final audit remain open.

## QUALITY repair development evidence

The installed assignment-event assertion failed before the reporting repair at
`lifecycle-model-switch-model-switch-quality-q2-assignment-red-20260913T111249529159Z`:
the actual event contained `opus`, while the source established `claude-opus-5`.
The exact pre-run source manifest is
`model-switch-quality-q2-red-source-20260913T111244112387Z/manifest.json`, SHA-256
`b1bcd7c8daa1feb37a9d4fe6f2051107cdd1e7feb367a77e3b0ec9a56c2969cc`.
The parent verified its seven retained source copies and the RED raw log hash.
The corresponding installed one-test GREEN is
`lifecycle-model-switch-model-switch-quality-q2-assignment-green-20260913T111318557624Z`.

The explicit integration-fixture rejection repair passed all 69 command-runner
tests at `lifecycle-model-switch-model-switch-quality-q1-plugin-command-runners-20260913T111333953124Z`.
All-target compilation with `--no-run` passed at
`lifecycle-model-switch-model-switch-quality-all-targets-no-run-20260913T111517332311Z`;
that result establishes compilation, not a full runtime test pass. The parent
checked these raw counts and log hashes. Corrected independent review and the
full regression remain required.

The worker released the corrected source. Only `workflow/mod.rs`, the existing
command-runner fixture, the installed owner test, and its event-collection support
changed for these repairs. The event collector now retains assignment events;
the installed assertion checks denial, allowed alias, canonical no-op and next
Sonnet selection against actual source and request models. Formatting, library
Clippy with warnings denied, and diff check passed at quality suffixes
`fmt-check-1-20260913T111510742323Z`, `clippy-final-20260913T111529564216Z`, and
`diff-final-20260913T111538128832Z`, with raw hashes verified by the parent.
Fresh `model-switch-quality-final-quality-delta.xml` and
`model-switch-quality-final-test-gate.xml` remain exits 2 and 4. They report
292 quality findings (108 worsened existing, 184 new, 38 minor, 70 gating,
zero acknowledgments) and 52 mapped tests with 596 untested symbols among
1,319 impacted. The mapped count changed with this candidate; the planned broad
regression still retains the earlier obligations. These are unwaived static results.

## Broad regression interruption and startup/cap repairs

The next broad run, `lifecycle-model-switch-all-targets-final-20260913T112013534461Z`,
was interrupted without a final exit. Its original metadata remains untouched.
The retained log has 29 completed suite summaries: 874 passed and 21 failed,
plus further individual failures without a completed suite summary. These are
partial counts. Separate observation `model-switch-interrupted-regression-20260913T123419021209Z.json`
records the absent processes and log SHA-256
`31a3199afec01e4d7fb87ae1a65b65c2f2e088ee48805ca260600788e17ed854`.
Later sequenced checks did not run. No native/compiler crash is established.

Independent QUALITY confirmed Q1/Q2 resolved but identified Q5/Q6. Its retained
reassessment has SHA-256 `83867007a80aa47a85bec38da823dd88e7da51aa17f47552fa83eeb9b40e1498`.
The parent read the complete report and recorded these findings before repair:

- Q5: ordinary authenticated Claude initialization now unconditionally requires
  a resolved model, breaking existing Oracle, compaction and async paths. Keep
  that knowledge optional for ordinary startup and require authentic facts only
  for live-switch/no-op eligibility. Missing facts must never fabricate identity,
  acknowledge source permission or bypass required source policy. Inspect catalog
  handling for the same compatibility boundary and preserve subscription checks.
- Q6: the retained Settings MCP-cap test changed its limit from two to three
  calls but still expects the third call refused. Restore the intended two-call
  fixture cap, preserving its final denial, unchanged bytes, single initialization
  and original allowance assertions. The observed third call is allowed by the
  configured limit; it does not prove a production capacity reset.

Required verification includes ordinary initialization without model knowledge,
missing-knowledge switch/no-op refusal, the exact Settings exhaustion scenario,
installed genuine-switch regressions and a completed broad run. The latest graph
dropped the directly edited command-runner target; its disappearance does not
remove that test obligation. All earlier 53 mapped obligations remain required.

## Q5/Q6 correction submitted for review

Ordinary authenticated Claude startup now leaves missing model knowledge absent.
ModelSwitch catalog handling applies only when its callbacks are configured.
Existing live-switch/no-op readiness and required-source policy holds remain.
The retained-cap fixture again uses two calls. The existing callback `bounded`
helper moved before its test module to satisfy all-target Clippy; its body did
not change. The worker released source and Cargo ownership without committing.

Q5's exact Oracle RED `lifecycle-model-switch-model-switch-q5-ordinary-auth-red-20260913T124444022950Z`
failed with missing resolved model; GREEN `...q5-ordinary-auth-green-20260913T124545629318Z`
passed. Q6's exact replacement/cap RED `...q6-retained-cap-red-20260913T124451970495Z`
returned Applied on the third call; GREEN `...q6-retained-cap-green-20260913T124558990571Z`
passed with the original denial assertion. Each selected one test. The parent
verified their raw counts and log hashes. Ellipses use the same
`lifecycle-model-switch-model-switch-` prefix.

The pre-RED manifest `model-switch-q5-q6-red-source-20260913T124402392308756Z/manifest.json`
is an inventory, not source copies. All 17 entries were independently matched
by the parent against actual retained bytes in the original `111742574615Z`
full-tree archive. That earlier immutable archive preserves the executed source.

Explicit ordinary malformed-catalog and missing-model switch/no-op controls
passed at `...q5-malformed-catalog-release-20260913T130146866151Z` and
`...q5-missing-knowledge-release-20260913T130151565280Z`. These direct passing
controls have no separately claimed RED. The latter retains the source-policy
hold without gate calls, fabricated assignment or changed identity.
Affected host-guard, compaction, async, hook-model and Claude post-tool cases
passed; the pinned installed group passed 13 tests at
`...q5-installed-claude-group-20260913T125456162005Z`. The parent checked their
raw counts and hashes. The complete commands and remaining evidence are recorded
in `model-switch-startup-repair-handoff.md`, SHA-256
`6ffc353743949a0c27e562380b428acb998a5831775e078275b5bc4510d310b6`.

All-target Clippy with warnings denied and all-target compilation passed at
`...q5-q6-clippy-final-20260913T130035815510Z` and
`...q5-q6-all-targets-no-run-final-20260913T130057920176Z`. Formatting and diff
check passed. Compilation is not a full runtime pass. Fresh static outputs
remain exits 2 and 4, with 296 quality findings/71 gating and 52 mapped targets/
596 unmapped impacted symbols. These remain unwaived. Corrected independent
reviews and a completed full runtime run are still required.

## Completed broad run: one compaction failure remains

Affected SPEC passed Q5/Q6 on freeze `130815844080Z`; its report is
`model-switch-startup-spec-review.md`, SHA-256
`ebba7b2ae68c3c716b2c0aed1aee21486578147ccb0f7191f22fdd2c57adc52c`.
The next full Rust run `lifecycle-model-switch-all-targets-final-20260913T131328519784Z`
completed with exit 101: 1,115 passed, one failed and 36 ignored across 48 suites.
Raw log SHA-256 is `037de44d0ee60f2fb7338ad71eb1d6764c89b5793147846f6667f6ea6f069ebe`.
Later sequenced checks did not run. The exact failing integration executable is
preserved under `model-switch-compaction-failure-20260913T132656441966Z` before
any diagnostic rebuild. This is an assertion failure, not a native crash.

The sole failure is `compaction_trigger_matching_prompt_and_agent_keep_unfunded_and_exhausted_holds`:
native Prompt/PostCompact/auto/matching/exhausted observed zero summaries instead
of one. The actual Pre request occurred. The fixture captures an error but omits
it from this assertion, so the reason execution stopped is not established.
Each case has a fresh 60-second allowance; the overall suite duration does not
prove that this case expired. Unchanged fixture/compaction files do not exclude
shared-runtime causation.

Independent QUALITY passed the Q5/Q6 code repairs but withheld final qualification.
Its immutable report `model-switch-quality-review-20260913T132817484781Z.md` has
SHA-256 `01f29d27eb5a0161ff0fc5428291b16fdeb49c7b8e3dc89be1eab431195397d7`.
The parent recorded the failure before diagnostic changes. Capture the omitted
error and relevant receipt/deadline state in a targeted reproduction. Preserve
the expected counts and existing limits; do not infer a transient failure or
raise timeouts without identifying the cause.

## Compaction failure diagnosis: not reproduced

The retained exact executable passed the case before any rebuild at
`lifecycle-model-switch-model-switch-compaction-retained-exact-20260913T132939903121Z`.
With failure diagnostics added, the exact case passed and the complete
`plugin_model_runners` target passed 38 tests at
`...compaction-diagnostic-target-20260913T133304718096Z`. A temporary diagnostic
print observed normal summary application and the expected Post hold after
5.495 seconds, with about 54.5 seconds of allowance remaining. That print was
removed; the final exact case passed again. These passes do not explain the
original failure, which remains unclassified and preserved.

Only `tests/plugin_model_runners/session_allowance.rs` changes: its failure
message includes the captured error, elapsed time, durable compaction state and
before/after allowance. Counts, deadlines and production behavior are unchanged.
Formatting, target Clippy and diff check passed. The handoff is
`model-switch-compaction-diagnosis-handoff.md`, SHA-256
`4afce30f6775dd7e930bdd7d7e94b7bd30e1adb7d84c203f0ef8b9ed9e9dd60e`;
the retained patch SHA-256 is
`9333c6ce2e08d1cbe491f35fd0a92f70752b8abc41d5bedb0f3b8c110874cbd3`.
Independent review and a full rerun must assess qualification without claiming
that the earlier failure's cause is known or adding automatic test retries.

## Final bounded qualification and integration

The final tested candidate is `lifecycle-model-switch-source-freeze-20260913T133831469782Z`,
based on `a71cc4987a42bad4f694086d7d545b1bb2cf2605`, with 2,833 retained paths.
Manifest SHA-256: `ac836f44b250c4369fb5fd81d486d34971f583f0360fbeb0f14f35fdb860a618`.
Archive SHA-256: `dea43f40973a5f0c40d3962530337d393d41e7e31c6e58b17be13f5aa85541e5`.
The retained application, `<freeze>-demoncoder`, has SHA-256
`13ea9fdb552930aa242f7bb9e9ba91557ad35fd1cbf9b1e1fc597d89ef374b5d`.
All artifact names here are under `/home/shawn/demoncoder-check-tmp/`.

The full command `cargo test --locked --all-targets --no-fail-fast` completed
with exit 0: **1,116 passed, zero failed and 36 ignored across 48 suites**.
Raw prefix: `lifecycle-model-switch-all-targets-final-20260913T134143110066Z`;
log SHA-256: `4d1aec759e63ff903eba5248ba40b565cee00f03e9c98bde41100f15a9898728`.
The previously failing compaction case passed without a production change,
relaxed assertion, longer allowance or automatic retry. Its earlier failure
remains unexplained, with the exact failed executable and logs retained.

| Final check | Actual result | Raw artifact suffix after `lifecycle-model-switch-` |
| --- | --- | --- |
| All-target Clippy with warnings denied | Exit 0 | `clippy-all-targets-final-20260913T135312992520Z` |
| Formatting and diff check | Exit 0 each | `fmt-final-20260913T135320407697Z`, `diff-final-20260913T135321499401Z` |
| Installed owner matrix | 15 passed | `installed-owners-final-20260913T135321955571Z` |
| Installed shared batch and Claude callback | One passed each | `installed-batch-peer-final-20260913T135503241339Z`, `installed-claude-batch-callback-final-20260913T135515851414Z` |
| Existing external Submit/Stop transitions | Eight passed | `external-transition-baselines-final-20260913T135516794911Z` |
| Retained application build | Exit 0 | `build-app-final-20260913T135606563711Z` |
| Provider/agent Settings | 13 passed | `provider-agent-settings-retained-final-20260913T135607294679Z` |
| Role Settings | Six passed | `role-settings-retained-final-20260913T135625607198Z` |
| Live Settings | Six passed | `live-settings-retained-final-20260913T135638567441Z` |
| Output limits | Three passed | `output-limits-retained-final-20260913T135642217484Z` |

The eight external cases use each pinned installed backend for repeated success,
Submit denial, Stop correction and cancellation. Actual request counts are
2/0/2/0 respectively. The parent and reviewer verified every case's nonempty
Rust result and all 509 retained-file hashes. Their manifest is
`model-switch-external-regression-20260913T135516819639Z/manifest.json`, SHA-256
`57b352b50f0fdd3bc2f48d1dd81f9be5f752124bc4f5f9a33b2be5cd70845efc`.
Installed checks use controlled local peers, not commercial-provider traffic.

Checkpoint `model-switch-final-checks-20260913T134142524079Z.json` preserves a
failed final comparator: two test-generated Python bytecode files were added,
with no original file, HEAD or external-input changes. The parent retained both
exact outputs in `model-switch-generated-bytecode-20260913T135714370316Z/`, removed
only those files, and ran the separate comparator
`lifecycle-model-switch-final-freeze-after-generated-output-cleanup-20260913T135714554292Z`.
It passed all 2,833 paths and external inputs with no additions or differences.
The original failed checkpoint was not rewritten. No behavioral rerun was needed.

The parent audit `model-switch-parent-final-qualification-audit-20260913T135734481517Z.json`
binds actual command exits, nonzero counts, raw metadata/log hashes and both
comparator results. Its SHA-256 is
`7c97a0463a9198d93fb8e680828c7d59b384aab30021f3ff7b936ec467a4d648`.
The final immutable QUALITY report is
`model-switch-quality-review-20260913T135922516883Z.md`, SHA-256
`2894b8ac7f6668aad9aeeb39aee90de48a1cb6dd4fb3131e61605098c90828c0`.
It approves the bounded implementation and development qualification after
independently inspecting the raw results, retained application and final freeze.
The earlier SPEC PASS remains applicable; production did not change afterward.

Parent production self-audit: the captured selection, actual identity, original
authority and durable uncertainty rules have matching implementation and failure
controls. The changes reuse existing admission, storage and runner mechanisms;
no new grant or replay path was added. Boundary validation, protected input,
cleanup, bounded waits, cancellation and recovery have focused controls and
completed regressions. The plan keeps exactly one task in progress. Documentation
and qualification claims distinguish tested behavior, ignored cases, source probes
and remaining obligations. All 14 production rules were checked; no required
repair remains for this bounded change. The minor duplication/complexity notes
and graph coverage limitations remain recorded, without suppressing the static
exit-2/exit-4 results or claiming those tools passed.

The developer explicitly requested committing, merging and pushing verified
progress before cleaning stale worktrees. This authorizes integration before the
whole commitment is Done. It does not authorize marking the compound lifecycle
item, public package workflows, full conformance or live-provider obligations
complete. Final report/plan edits after the tested freeze are documentation only.
