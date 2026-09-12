# Skills, plugins and hooks implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development for bounded implementation tasks, with specification and quality reviews. The selected Cairn commitment and its falsifiers remain authoritative.

**Goal:** Deliver every requirement of the selected 41-requirement commitment on all four connections.

**Architecture:** Import packages without executing them, retain immutable generations, and attach one typed plugin runtime to the existing tool and workflow boundaries. Plugin effects use existing confinement, cumulative allocation and durable operation receipts. External backends own their model loops but must await the host's authenticated lifecycle decision before crossing a guarded boundary.

**Tech stack:** Rust, Tokio, Serde, the existing descriptor-based storage and Linux confinement primitives, pinned upstream wire/schema inventories, controlled Python transport fixtures, and installed backend qualification.

## Status and execution rules

- Complete: Immutable import foundation, backend compaction qualification probes, wire validation, result decoding, gate snapshots, durable tool receipts, pre-tool final-candidate admission, the confined command-runner/snapshot-materialization prerequisite, bounded PreToolUse prompt/agent runners, the admitted HTTP runner prerequisite, managed MCP service/hook admission, durable synchronous one-shot activation and recovery, and owned asynchronous observers with parent and child rewake.
- **In progress:** Complete lifecycle dispatch.
- Pending: Integrate state, recovery, services and all package components.
- Pending: Exercise the complete public management and coding workflows.
- Pending: Run full conformance, installed/live cases, adversarial review and Cairn checks.

Earlier qualified lifecycle prerequisite: native and external Submit/Stop framing and
ownership are verified. The final shared/Codex integration passes 815 Rust tests
(17 ignored), 43 actual backend/local-peer cases (21 Codex, 22 Claude), 30 host
compaction cases, and fresh specification and quality reviews. Source qualification
passes 191 hook tests, 25 startup and 43 ordinary runtime cases, three lifetime
cases, 53 compaction cases and four model-isolation cases. The full-context and
controlled-peer fixes retain their negative controls and earlier failed runs.
Complete lifecycle/package conformance remains open. See
[the integration review](../reviews/plugin-codex-external-non-tool-runtime.md).

Latest verified prerequisites add native provider-failure observations, original
native session lifetime ownership, the application shutdown reservation, explicit
session-hook allowance persistence with atomic session directory creation, exact
operation budget/usage attribution including ordinary Oracle ownership, and native
session Prompt/Agent execution from the original explicit grant. The current Rust
candidate passes 938 tests (17 ignored), the separate terminal output-limit suite
passes three cases, and fresh specification and quality reviews pass. Earlier
installed/live-source evidence retains its original candidate; these Rust checks
do not refresh it. See the [model allowance review](../reviews/plugin-session-model-allowance.md)
for corrected authority findings, static non-passes and evidence-capture limits.
The [attribution review](../reviews/plugin-operation-budget-attribution.md) retains
the earlier Oracle findings and unexplained crash.

Only the parent records Cairn decisions, changes declarations and commits evidence. Workers own bounded files; they do not change the selected scope or evidence. A test fixture may model a provider, but its report must say controlled transport. Source/type checks, successful parsing and test registration never establish runtime delivery.

The implementation worktree is on `feature/skills-plugins-hooks`. The source references remain read-only. Source copies or patches introduced into the deliverable retain their revision, hash and license. Add newly created package/backend artifact paths to the mechanism's inputs before checking them.

## 1. Immutable import and inspection foundation

**Files:** Create `src/plugins/{mod.rs,snapshot.rs,manifest.rs,types.rs}`, `tests/plugin_import.rs`; export the module from `src/lib.rs`. Public management integration later uses this same API, not a second importer.

The initial interface is:

```rust
pub enum Dialect { Claude, Codex, Portable }
pub struct ImportOptions { pub dialect: Option<Dialect> }
pub fn inspect(source: &std::path::Path, options: &ImportOptions)
    -> anyhow::Result<Package>;
```

`Package` retains source identity, digest, exact admitted files and an inspection report. The report distinguishes source-shape validity, executable readiness and activation; inspection never activates. Canonical relative file names are the only component references. Each file includes bytes, kind/mode and content hash. Never reread a source path to execute a previously inspected generation.

- [x] Add `tests/plugin_import.rs` tests for three dialects, root plus overlay, inline replacement, malformed inline objects, independent legacy manifest ambiguity and manifest-free Claude discovery.
- [x] Run `cargo test --locked --test plugin_import`; the missing public importer must fail compilation before implementation.
- [x] Implement bounded descriptor-relative snapshot reads: 4,096 entries, 1 MiB per declaration/text file and 32 MiB aggregate; reject outside traversal, special files and unsafe links. Race-safe source reads must detect changed file metadata/content. Later binary assets use separately declared limits instead of silently increasing declaration limits.
- [x] Implement manifest composition using PCOMP-001, canonical-file deduplication, qualified identity collision checks and field-level diagnostics. Unknown executable declarations cannot be reported executable. Preserve metadata without treating it as instructions or permission.
- [x] Add malicious-path, source-mutation, oversize, duplicate-name and discovery-canary tests. Confirm source edits cannot change captured bytes and malformed required components prevent readiness.
- [x] Run the integration test and `cargo clippy --locked --all-targets -- -D warnings`; review specification compliance, then code quality, before committing.

Foundation verification: 34 importer integration tests, 3 deterministic snapshot mutation tests and all-target Clippy passed. Specification and quality reviews found and resolved empty-directory discovery, dialect diagnostics and duplicate-path resource amplification. Execution remains unavailable until the later runtime stages.

## 2. Actual backend barrier qualification

**Files:** `src/plugins/bridge.rs`, `src/adapters/{claude.rs,codex.rs,process.rs}`, `tests/plugin_backend_barriers.py`, controlled endpoint helpers in `tests/`; a pinned Codex patch/build record under `backend-integrations/codex/` if required.

The relay envelope has explicit `version`, `connection`, `session`, `task`, `sequence`, `callback`, `event`, `operation`, `deadline` and candidate digest fields. Authentication comes from an inherited private host channel, never the event payload. A result matches one envelope and one candidate; an after-action notification cannot answer a pre-action request.

- [x] Exercise installed Claude's `initialize.hooks` callback registration and a real manual compaction against a local model peer. Capture allow and denial, checking backend state and model requests independently of callback text.
- [x] Trigger actual automatic compaction and repeat allow/deny. Kill, disconnect and time out the callback path while it is waiting; observe whether the backend crosses the boundary. Keep the owner alive in the disconnected-relay case so the probe does not merely prove killing all processes stops work.
- [x] Implement the owner-supervised Claude control lifetime and correlated cancellation needed to hold failed barriers. Retest all failure cases through the production adapter.
- [x] Probe pinned Codex manual and automatic compaction using an isolated hook configuration. If the command runner fails open, patch the exact managed relay boundary in the pinned source. A normal upstream hook must not acquire global behavior changes accidentally.
- [x] Build and qualify that managed Codex artifact; record source digest, patch digest, toolchain, command and executable digest. Test forged/stale/duplicate acknowledgments and ambient plugin canaries.
- [ ] Register only qualified versions for plugin-dependent execution. Any demonstrated proprietary limitation becomes a concrete Cairn escalation, not a capability warning counted as delivery.

Backend development checks: managed Codex 179 hook tests, 36 actual backend fault cases, 17 startup cases and 30 production-adapter cases pass. Installed Claude has 18 compaction cases and two abrupt-owner-death cases passing. Controlled lifetime tests additionally cover supervisor death, partial input, output backpressure and descriptor ownership. See [the qualification review](../reviews/plugin-backend-qualification.md) for corrections and evidence limits. Public activation still must enforce the qualified version policy.

## 3. Runtime wire validation and source semantics

**Files:** `src/plugins/{wire.rs,profile.rs,hook_types.rs}`, `tests/plugin_wire.rs`, runtime coverage data under `tests/fixtures/plugins/`.

- [x] Compile the frozen JSON schemas with external retrieval disabled. Resolve only the retained schema/reference closure. Validate Claude's nested graph and every union alternative using the recorded event/control roots.
- [x] Implement the exact 510-cell applicability lookup; missing cells and unsupported source pairs fail validation without a native fallback.
- [ ] Add the explicit developer conversion control that creates a separately validated native declaration with its required runner settings.
- [x] Implement event-specific command/HTTP/MCP responses and separate prompt/agent schemas. Retain ignored source fields as ignored. Worktree paths, watch updates, elicitation and display responses keep their special meaning.
- [x] Generate positive and negative shape cases per reachable field/branch. Controlled nested-field and union-branch mutations must break unchanged production fixtures.
- [x] Exercise typed effect cases per event and remove one nontrivial response-effect handler in a controlled mutation test; it must fail. Actual host effects remain integration work.

The [wire foundation review](../reviews/plugin-wire-foundation.md) records the
closed schema-coverage and string-allocation findings, independent reviews and
executed checks. The foundation interprets model outcomes but does not execute
those outcomes or the command/HTTP/MCP event effects.

The [result decoder review](../reviews/plugin-result-decoding.md) records 49
decoder and 34 importer tests, a failing watch-effect deletion probe, the corrected
Codex async and continuation semantics, and both independent approvals. These
results establish typed proposals; the following stages must execute and retain
their actual effects through the host runtime.

## 4. Final-candidate admission and durable lifecycle

**Files:** `src/plugins/{dispatch.rs,admission.rs,receipts.rs}`, `src/tools.rs`, `src/events.rs`, `src/workflow/{runtime.rs,state.rs,store.rs}`, `tests/plugin_admission.rs`.

Integration constraints from the existing production paths:

- `EventSink::emit` retains workflow state before sending UI events. Add invocation and admission records through `SharedRuntime`; do not introduce a separate allowance or effect ledger.
- `main` opens `SharedRuntime` before opening the adapter, including ordinary sessions with no verification task. Attach startup plugin records there. A new record has no allocation; admit model-backed hooks only after attaching the appropriate cumulative allowance. External-backend conversation restore is currently refused, so plugin reconciliation must not claim it restores that conversation.
- `ToolExecutor::execute` retains the actual result in its in-memory `completed` slot before its next await. Language diagnostics can await before `ToolFinished` reaches durable storage. Plugin integration must retain the original result durably before any observer await, while keeping model-facing replacements separate. Native interruption recovery consumes that slot; external adapters do not currently do so.
- `workflow::workspace::capture_scoped_cancellable` already captures bounded dirty and untracked bytes using two descriptor-relative scans. Extend its revision with ownership and ACL identity, preserving historical snapshot decoding. Snapshot materialization currently lives in `subagents::worktree`, not `workflow::review`.
- Verification's `CaptureScope` excludes declared generated outputs during capture. A gate read set must be resolved from its own policy; do not silently inherit those review exclusions for the legacy whole-workspace default. Protected source exclusions still apply, and an oversized required snapshot holds the gate.
- The executor currently relies on sequential execution and has no shared mutation lock. Add a host-owned serialized boundary for final freshness checks and mutation, shared by relevant executors. A host lock cannot provide atomicity against external writers; stronger policies need a supported transaction or a hold.
- Bind that boundary to the admitted workspace identity. Isolated child worktrees must retain independent execution; a long Bash operation in one child must not hold a process-wide lock over every other child's workspace. Run gate inspection before acquiring the mutation boundary so a gate's read-only tools cannot deadlock behind their caller.
- `ToolStarted` currently allocates another operation for every event; it does not deduplicate a repeated backend call ID. Plugin admission needs a durable operation identity and replay lookup before effects, with backend IDs retained as scoped correlation data. Duplicate UI publication must not allocate another invocation or overwrite the original result.
- Native restore currently feeds `Operation.result` directly back to the model for operations after the checkpoint cursor. Retain the original tool receipt and any admitted model-facing replacement separately, and restore the same admitted presentation without rerunning observers. Historical records without a replacement retain their existing result behavior.
- Scope backend call IDs to a durable host invocation, not merely the `worker` phase. Native model admissions already have IDs, but `EventSink::begin_backend` records an admission only for delegated sessions. Ordinary external turns need an explicit identity too; a reused source call ID in a later turn must not select an earlier turn's receipt.
- Verification and both subagent validation paths call `ToolExecutor` directly. Give these command groups host invocation identity while retaining their check attribution and existing allowance rules; they are not model invocations.

- [ ] Persist invocation identity and causal source before effects. Implement transformer, decision, observer and legacy-combined classes. Native priority and source concurrent groups retain their specified ordering.
- [ ] Freeze the candidate after at most four revisions; recompute applicable matchers, retain every deny and reject conflicting concurrent rewrites. A stale combined decision requires a declared read-only endpoint or a visible hold; never rerun its effects.
- [ ] Capture gate read sets, including absent paths, file kinds, symlink targets, modes, ownership and ACLs. Gates inspect an immutable dirty/untracked admitted snapshot. Rescan before release and compare inside the host mutation boundary.
- [ ] Bind developer answers to the frozen candidate and authenticated control origin. Input, configuration or permission changes invalidate pending answers.
- [ ] Test actual write outcomes for both handler orders, delayed stale results, permission revocation, uncertain admissions and cancellation. Preserve original operation receipts before observers or display transformations.

The [gate snapshot prerequisite](../reviews/plugin-gate-snapshots.md) is implemented
and independently reviewed: 54 affected integration tests and nine workspace unit
tests pass. Review exposed and corrected retained ACL-buffer allocation beyond the
metadata allowance. Gate runners, metadata-preserving materialization, final
admission and mutation-boundary comparisons remain integration work, so the gate
capture obligation above is not yet complete.

The [durable tool receipt prerequisite](../reviews/plugin-tool-receipts.md) is
implemented and independently reviewed. Actual host invocation IDs now scope
retries across all four adapter paths and direct verification commands. Original
results precede observer awaits; known denials, unknown effects and settled
model-facing output remain distinct. Review corrected access-denied observers,
budget-denial compatibility and a post-persistence clock gap. Final affected
checks passed.

The [final-candidate admission prerequisite](../reviews/plugin-final-candidate-admission.md)
is implemented and independently reviewed. Host-selected pre-tool plans now run
the bounded rewrite, decision and concurrent-group protocol through ToolExecutor,
retain hook receipts, and validate frozen inputs inside the shared workspace
mutation boundary. Review corrected equivalent-path matcher bypasses and lost
completed outcomes after sibling uncertainty. The corrected candidate passed
250 tests, with one explicitly ignored external-assessment case; independent
specification and quality checks also passed. Controlled runners exercise this
path. The five production runners, activation, developer answers, extension/LSP
integration and remaining lifecycle events still need integration, so section 4
and the complete commitment remain in progress.

## 5. Five confined runner types

**Files:** `src/plugins/runners/{mod.rs,command.rs,model.rs,http.rs,mcp.rs}`, `src/worktree_access.rs`, `src/workflow/workspace.rs`, `src/subagents/worktree.rs`, `tests/plugin_runners.rs`.

`AccessPolicy::review_only` disables every tool; it is suitable for a prompt hook,
but cannot supply an agent hook's read-only inspection tools. Add an explicit
snapshot inspection policy and enforce it in both the tool boundary and process
confinement. Keep hook phases distinct for usage attribution while retaining an
owning `agent:<id>` prefix, which the runtime uses to reject stopped assignments.

Runner integration constraints from the existing production paths:

- `WorktreeAccess::command` currently mounts its workspace writable and uses
  stdin as the root descriptor. Command hooks need a separate bounded JSON stdin
  stream and explicit descriptor mounts. Decision runners mount the retained
  snapshot read-only; transformer grants remain separate. Preserve the existing
  credential masks, cleared environment and socket confinement.
- `subagents::worktree::materialize` copies bytes and ordinary mode bits, but does
  not reproduce captured ownership, ACLs or other access metadata. Gate snapshot
  materialization must preserve the required inspection semantics or hold with
  a specific reason. A byte-identical copy alone does not establish that proof.
- `supervisor` owns orphaned Bash descendants with a subreaper and lifetime pipe.
  Backend supervision instead uses a process group and a stdin lease. Select and
  adapt ownership for arbitrary hook commands deliberately; demonstrate cleanup
  for detached descendants, cancellation, output overflow and owner death.
- Prompt runners can follow `workflow::review::run_prompt` for tool-free adapter
  execution and closure, but need their own response schema and owning allowance.
  Agent runners need snapshot inspection tools. Neither runner may substitute a
  different configured model or recover authority from response-supplied identity.
- `begin_model_as` permits ordinary sessions without an allocation. Model hooks
  must explicitly require the owning task or configured session allowance before
  opening a model request. External backend admissions currently use a separate
  delegation invocation limit; hook integration must account for that path too.
- External backend admission currently records a backend invocation without
  admitting a model call against the ordinary task allocation. Model-hook
  integration must require and charge its owning allowance atomically on that
  path too; a preflight balance check alone is insufficient. Preserve ordinary
  sessions while making hook authority explicit in the host event context.
  SUB-006 distinguishes native model calls from external backend invocations:
  charge each hook backend invocation under its stated controls, preserve finite
  deadlines and tool limits, and retain unknown internal request counts. Never
  describe an invocation cap as an enforceable backend-internal model-call cap.
- Usage attaches to an incomplete operation by phase. Give concurrent hook model
  invocations distinct causal phases, retaining any `agent:<id>` owner prefix.
  Do not reuse a generic reviewer phase or forward usage twice when presenting it.
- Generic workspace capture applies the static export exclusions, while an
  executor can also have custom credential paths. Model-hook evidence must apply
  those actual host exclusions before exposing captured contents. Command mount
  masks alone do not protect a prompt assembled from snapshot bytes. Revalidate
  credential aliases before model input delivery too; a final freshness hold
  after sending the prompt cannot undo an earlier disclosure.
- Snapshot-backed agent tools must keep logical workspace paths bound to the
  retained snapshot, including absolute in-workspace symlinks. Both direct reads
  and confined commands must use that view; opening the live workspace through
  an ordinary executor would invalidate the inspection guarantee.
- The provider HTTP client disables redirects, but its response helper discards
  non-success bodies. Hook HTTP runners decode bounded successful source-protocol
  responses; transport and non-success gate responses hold with secret-safe
  diagnostics. Check endpoint and credential authority before any redirect.


The [command-runner prerequisite](../reviews/plugin-command-runners.md) is
implemented and independently approved. Review closed private-file exclusions,
relative and changing credential aliases, and descriptor growth during snapshot
and package staging. The final broad suite passed 533 tests with sixteen explicit
ignores; independent specification and quality controls passed. The complete
runner family and lifecycle integration below remain pending.

The initial command prerequisite covers PreToolUse with network denied. Complete
command integration still includes declared network grants, every applicable
lifecycle input and source-configured timeout policies. Replace prerequisite
deadline constants with admitted handler deadlines bounded by the owning
allowance, while retaining cleanup time and unknown-outcome handling.

The [prompt/agent prerequisite](../reviews/plugin-model-runners.md) is implemented
and independently approved for PreToolUse. Model16, command30, library193 and
affected regression controls passed; both external backends have actual local
isolation and compaction evidence. Independent adversarial probes verified
verdict/inspection rejection and failure when managed isolation is removed.
Other lifecycle events and public activation remain pending.

The [HTTP prerequisite](../reviews/plugin-http-runners.md) is implemented and
independently approved for Native and Claude PreToolUse. HTTP24, the unchanged
external authority probe, three additional quality controls, affected runner
regressions and the actual registry terminal case passed. Review found and fixed
empty written URL authority normalization. The runner binds endpoint/header/
credential authority, preserves uncertain exchanges, and uses a separately admitted
read-only revalidation endpoint without repeating the primary POST. Public source
configuration, remaining lifecycle events and MCP integration remain pending.

The [managed MCP prerequisite](../reviews/plugin-mcp-runners.md) is implemented
and independently approved for PreToolUse. It binds stdio and Streamable HTTP
services to complete immutable authority, the original runtime owner and bounded
capacity. Discovery and schema validation precede hook calls; uncertain calls
never replay. Review found and fixed missing output-schema enforcement, buffered
duplicate-response release, and a traffic observer that missed bodyless GETs.
The final production suites passed 200 library, 23 MCP and 336 other integration
cases; sixteen installed/live/subprocess entries remained explicitly ignored.
Independent hostile-response controls and specification/quality reviews passed.
Public service/configuration/authentication and full lifecycle work below remain
part of the same incomplete commitment.

- [ ] Command: JSON stdin, explicit argv/shell, declared environment/access, bounded output/deadlines and owned descendants. Literal event values never enter executable shell source.
- [ ] Prompt: selected model, no tools, schema-validated verdict, cumulative admission and usage. Agent: immutable admitted snapshot, bounded read-only inspection, retained evidence and the same ledger.
- [ ] HTTP: explicitly bound endpoint/headers/credentials, redirects checked before disclosure, bounded response and cancellation. MCP: admitted service/tool and structured/text response handling, with `isError` as failure.
- [ ] Apply source-specific continuation, `impossible`, once, async and rewake semantics. Reserve once before effects, consume only after success; unknown outcomes remain reserved for reconciliation.
- [ ] Demonstrate private-canary denial, injection resistance, descendant cleanup, malformed responses, timeout holds, late observer outcomes and no hidden/unaccounted model calls.

## 6. Package catalog, activation and versioned state

**Files:** `src/plugins/{catalog.rs,state.rs,activation.rs,recovery.rs}`, `src/workflow/{runtime.rs,state.rs}`, `tests/plugin_activation.rs`.

The existing `workflow::store::Store` provides descriptor-pinned private storage,
exclusive ownership, bounded checksummed records and atomic publication. A directory
sync failure after replacement means publication is uncertain; callers must hold
execution rather than assume the old record survived. Reuse this behavior for
plugin records. Replacement-task activation needs a dedicated runtime transaction:
ordinary `archive` can clear the allocation, `allocate` creates a fresh allowance,
and `Task::new` resets the correction count. Preserve the original allocation and
spent corrections while moving future admission ownership to the replacement.
`SharedRuntime::update` mutates its in-memory record before the closure returns;
an error does not restore it. Validate replacement preconditions before mutation,
or stage a complete replacement record and publish it only after validation.
Test rejected transactions followed by another successful update, so a rejected
partial change cannot leak into a later persisted record.

- [ ] Use descriptor-pinned private storage and atomic durable publication. Persist installed, validated, enabled, disabled and quarantined state separately.
- [ ] Pin code/state/policy/configuration to tasks and children. Resolve managed, bundled, user, project and local-project scopes deterministically; names never merge identities or authority.
- [ ] Stage updates with prepared/migrated/validated/activated transitions. Quiesce writers, migrate only a staged copy and retain old referenced generations. Unknown effects do not replay.
- [ ] Implement non-hooked developer quarantine and explicit linked replacement-task recovery. Preserve settled effects, child worktrees and the cumulative ledger; atomically transfer future admission ownership.
- [ ] Test crashes at every transition, failed migrations, divergent generations, managed authority, duplicate recovery requests and exhausted ledgers. Rollback and removal retain referenced state and require explicit data-deletion intent.

## 7. Skills, commands, agents and workflows

**Files:** `src/plugins/{skills.rs,workflow.rs,context.rs}`, `src/subagents/{manager.rs,session.rs,state.rs}`, `src/learning/context.rs`, `tests/plugin_skills.rs`.

- [ ] Parse bounded YAML frontmatter without duplicate-key ambiguity or alias expansion attacks. Honor every inventoried skill/agent field and `agents/openai.yaml` invocation restrictions.
- [ ] Expose bounded qualified catalogs. Load selected bodies/resources with provenance. Reserve host command names and substitute invocation arguments literally.
- [ ] Route declared dynamic context commands through admission; route forks and agent templates through the existing confined assignment manager. Enforce selected model/tool restrictions and explicit integration.
- [ ] Deliver matching skill context on all four connections; test developer-only invocation, unknown fields, shell canaries, collisions, forks and cancellation through actual adapter requests.

## 8. MCP, LSP, connector configuration and credentials

**Files:** `src/plugins/services/{mod.rs,mcp.rs,lsp.rs,auth.rs,config.rs}`, `src/language_services/mod.rs`, `src/settings/`, `tests/plugin_services.rs`, `tests/plugin_connector.py`.

- [ ] Implement bounded stdio and Streamable HTTP MCP initialization, tool discovery/calls, elicitation and lifecycle. Reject dependency/bootstrap cycles; startup cannot recursively await itself.
- [ ] Reuse the filtered language-service boundary with deterministic registration/fallback. Service reuse compares complete code/configuration/state/workspace/role/credential identity.
- [ ] Add typed configuration forms with host-owned sensitive values. Bind registered apps to explicit package digest, endpoint, transport, account and allowed tools.
- [ ] Implement authorization discovery and browser PKCE with callback-state validation, endpoint identity checks and revocation. Secrets never enter package state, prompts or ordinary receipts.
- [ ] Test local authentication failures, redirects, cancellation, restart bounds and conflicting LSP registrations. Run the pinned Supabase authenticated discovery and read-only project-list case through the installed application; no project mutations.

## 9. Monitors and channels

**Files:** `src/plugins/{monitor.rs,channel.rs,delivery.rs}`, `src/session.rs`, `tests/plugin_delivery.rs`.

- [ ] Persist source generation, receive sequence, cursor, queue, delivery IDs and gaps. Separate acceptance, model delivery and resulting work admission.
- [ ] Stop line-only producers on overflow and preserve the unknown gap. Restart only after the developer acknowledges that gap; replay-capable sources resume from the durably accepted cursor.
- [ ] Authenticate channel sources and bind routing to workspace/package identity. Deduplicate stable IDs without forgetting retry identities at storage limits.
- [ ] Flood, crash, reconnect and restart sources around each acknowledgment boundary. Verify external command-shaped content cannot invoke developer controls and delivery never interrupts a mutation.

## 10. Real host lifecycle transitions

### Typed non-tool lifecycle ownership

Decision: [typed lifecycle operations](../decisions/bind-non-tool-hooks-to-typed-lifecycle-operations.md).
This is the next part of the active lifecycle item. All remaining lifecycle
families and source qualifications stay required in this commitment.
The [Claude source qualification](../reviews/plugin-non-tool-source.md) passed
three no-tool cases and 31 corruption checks, with independent specification
and quality approval. It establishes genuine SDK framing and source behavior;
production lifecycle delivery remains part of the unchecked work below.
The [Codex source qualification](../reviews/plugin-codex-non-tool-source.md) also
passed three cases and 30 corruption checks with both independent approvals.
Dialect does not select the backend: native occurrences use genuine host facts
when translated into imported schemas, while external occurrences use correlated
backend observations. Missing external facts must never be invented.

The [native runtime review](../reviews/plugin-non-tool-runtime.md) records
specification approval after child ownership and cleanup-oracle repairs. The
broad check had one test-helper failure; final affected checks passed after its
repair. Quality review then found incorrect event decoding in asynchronous
Submit/Stop completion and delivery. A regression reproduced the wrong once
outcome; its repair passed affected checks and both follow-up reviews. The
runtime review retains exact candidate hashes and test chronology. Remaining
tasks stay unchecked until implementation and required reviews are complete.

- [x] Add typed non-tool occurrences to the existing operation ledger and a
  shared reserved-hook view. Preserve old records, tool admission proofs,
  once reservations, uncertain effects and asynchronous owner traversal.
- [x] Dispatch actual UserPromptSubmit and ordinary Stop inside the owned
  worker turn. Keep blocked prompts inspectable and Stop corrections before
  stopped/phase completion, within the original correction and spending limits.
  Cancellation and shutdown must not enter or wait for a Stop correction.
- [x] Add the [durable native turn identity](../decisions/give-each-native-turn-a-durable-identity-before-hook-selection.md) before optional hook selection. Preserve it through Stop corrections and Stop-only configurations. Plugin-origin work records a real turn without fabricating a developer submission. Use this identity and real host transcript/model/policy for source-format translation.
- [x] Frame native events from host facts. Obtain actual source facts before
  claiming Claude or Codex framing; never fabricate a tool call, backend turn,
  transcript path or source session. Test command-shaped context as data.
- [x] Demonstrate rejection before model work, corrected-pass and always-block
  termination, cancellation, stale ownership and interrupted reservation recovery.
  Run affected regressions and independent specification then quality review.

### Native failed-turn observation prerequisite

Decision: [observe actual provider failures](../decisions/observe-native-provider-failures-before-ending-their-original-turn.md).
The next part of the active lifecycle item is native StopFailure for a returned
provider error while its original durable turn remains live. Preserve the error,
owner and remaining allocation; observer results cannot continue work or change
failure into success. Admission, hook, persistence and tool errors must not be
misreported as provider failures. Cancellation retains priority and observers
cannot recursively generate another StopFailure.

Implementation and verification passed for this prerequisite. Production tests
observe native provider failure, actual hook effects, failed/malformed observer
results, cancellation, no task continuation or model retry, and truthful handling
of expired ownership. Explicit model handlers retain their declared, charged use.
Outer timeouts that drop the turn future, session lifetime
ownership, Interrupt and actual external source events remain required later.
Fresh specification and quality reviews passed. The final full Rust suite passed
841 tests with 17 ignored and no failures; Clippy and formatting passed.
The [review](../reviews/plugin-native-stop-failure.md) retains the meaningful
failing controls, static findings and precise evidence limits. These development
checks do not replace the complete commitment's Cairn evidence.
The [error-category matcher](../decisions/match-failed-turn-observers-against-their-recorded-error-category.md)
uses the existing bounded regex implementation, is valid only for StopFailure,
and preserves older declaration bytes when absent. Shutdown during a failed-turn
observer closes and drains the existing command receiver so the outer session
still terminates while the original provider error is preserved.
Bounded diagnostics on the original turn must survive reopening and appear in
inspection, including when observation cannot execute or cleanup fails.
An actual MCP observer call exposed validation that always used the required-gate
role. The runner must validate against its admitted invocation role; tests must
retain strict gate checks and reject forbidden observer decisions.
Fresh review also found local event errors inside `Model::response`. The
[provider-origin marker](../decisions/mark-provider-response-failures-at-actual-adapter-transport-and-protocol-boundaries.md)
must distinguish actual transport/protocol failures from local output,
persistence and validation failures, preserving the original returned error.

### Native session lifetime prerequisite

Decision: [own startup and shutdown observations](../decisions/own-native-session-startup-and-shutdown-observations-without-borrowing-task-authority.md).
Verified: the native host session now owns a durable lifetime and synchronous
command observations at actual startup/resume and termination, including no-prompt
sessions. Resource close after cancellation, failure or provider replacement does
not invent SessionEnd. Exact identity, workspace and frozen plans govern effects;
task allowances and recovery holds remain unchanged. Typed termination causes
preserve explicit shutdown and channel closure without wrapping provider errors.

Startup has a 30-second asynchronous boundary and end has five seconds, including
tracked command cleanup. Real process, queued-control, replacement, uncertain
restart, persistence-failure and failed/cancelled-turn continuation probes pass.
Final all-target regression: 869 passed, zero failed, 17 ignored. Formatting,
Clippy, fresh specification review and fresh quality review pass. Static findings
and the synchronous-I/O timing limit are retained in
[the prerequisite review](../reviews/plugin-native-session-lifetime.md).

This does not complete lifecycle dispatch. HTTP/MCP/model session execution needs
explicit allowance and service/accounting ownership. Configured asynchronous
session commands need session-owned lifetime policy and remain unavailable here;
they are not silently converted to synchronous execution. Those behaviors,
additional startup effects, Interrupt, outer timeout and actual source events
remain in this same commitment.

### Native session model allowance integration

Decision: [fund models from the original session grant](../decisions/fund-native-session-model-hooks-from-their-original-explicit-grant.md).
Implementation and verification pass. Actual native startup and shutdown run
synchronous Prompt/Agent hooks only from their original explicit session grant.
Preparation, request/delivery, model/backend admission and snapshot tool effects
agree on the same live owner and budget reference. Final checks follow durable
transitions before effects. Original cumulative time and counters remain shared
across startup and end; existing occurrence boundaries and cleanup reserves apply.

Session backend invocations have a separate durable count, independent of unrelated
delegation counts. Ordinary task/delegation hooks retain their limits and cannot
borrow session funding. Persisted references cannot recreate execution authority.
The complete suite passes 938 tests with 17 ignored, Clippy and formatting pass,
and the separate PTY suite passes three cases. Fresh specification and quality
reviews pass after correcting post-persistence authority and legacy-shape findings.
The [review](../reviews/plugin-session-model-allowance.md) records evidence identities,
failed controls, static non-passes and the corrected evidence-capture process.
HTTP/MCP and asynchronous session command lifetime integration remain pending;
native host execution does not qualify external backend lifecycle sources.

### Exact operation budget and usage attribution prerequisite

Decision: [pin accounting to its original invocation](../decisions/pin-operation-budgets-and-settle-usage-by-exact-invocation.md).
Implementation and verification pass. New causal owners and model/backend/tool operations
retain an explicit original budget reference, including an unfunded state. Task
allocation epochs distinguish replacements even with equal limits and timestamps.
Late and unknown usage settles by exact invocation into the original retained
allocation; retired accounting cannot admit execution. A bounded set of 32
referenced retired allocations preserves taskless replacement history. Existing
archive values remain historical snapshots, and unprovable legacy attribution is
visible without assigning it to a newer grant. Arithmetic and retention-limit
failures preserve existing state. Ordinary Oracle review retains its pending tool
across delayed event delivery, allowing normal completed Model/Commands sources
while rejecting replacement funding. The corrected parallel regression passes
919 tests (17 ignored), with Clippy, formatting, three terminal cases and fresh
specification and quality reviews passing. This action repairs attribution on
current execution paths; session model and service runner enablement follows.
See [the review](../reviews/plugin-operation-budget-attribution.md) for proof and limits.

### Explicit session-hook allowance persistence prerequisite

Decision: [persist an explicit session grant](../decisions/persist-explicit-session-hook-allowances-without-task-funding-or-resume-resets.md).
Implementation and verification pass. Three explicit invocation options configure the
session's cumulative time, model/backend invocation slots and host-observed tool
slots; omission grants no allowance. The existing durable runtime stores it
separately from task/delegation funding. Resume must retain the exact limits,
counters, deadline and uncertainty; changed, removed or newly added options must
fail before recovery state is changed. Existing task limits remain unchanged.
This first action verifies persistence and public configuration only: focused
controls, the 894-test parallel Rust regression (17 ignored), three terminal cases,
formatting, lint and fresh specification and quality reviews all pass.
Exact operation attribution, model/service admission, spending and lifecycle
runner execution follow as separate reviewed actions. No new handler is enabled
by this persistence prerequisite.

The full parallel regression exposed an existing timestamp/process-ID session
directory collision. The [atomic creation repair](../decisions/reserve-unique-session-directories-atomically-under-concurrent-startup.md)
is verified as part of this prerequisite: fresh bounded candidates preserve
existing records and later initialization failures propagate without retry.
The actual 32-open probe changes from two successes and 30 collisions to 32
successes. The default-parallel regression passes, with the original failure
retained in the review.

### Application shutdown deadline integration repair

Decision: [reserve the native observation budget](../decisions/reserve-outer-shutdown-time-for-native-lifetime-observations-and-resource-cleanup.md).
Follow-up inspection of `main` found its three-second worker-abort deadline can
cut across the five-second native end observation budget. The repair is verified:
the application preserves three seconds of ordinary cleanup and reserves the
native five-second bound for initially native sessions, including after replacement.
External-only sessions keep three seconds. Tests exercise the exact helper main
calls with a confined command, delayed resource cleanup, queued quit, worker errors
and joined timeout aborts. The full Rust run passes 873 tests (17 ignored), formatting
and lint pass, and fresh specification and quality reviews pass. The review records
static diagnostics and remaining evidence limits. This does not complete lifecycle
dispatch or replace the remaining session-allowance work.

### Durable synchronous one-shot prerequisite

Decision: [activation and exact outcomes](../decisions/bind-one-shot-hooks-to-durable-activation-and-exact-outcomes.md).
Build this before async ownership; both remain required in this commitment.
The existing session store and hook receipts provide persistence and ownership.
Package generation cannot serve as a skill invocation counter: it also pins MCP
service identity. Add a bounded host-recorded activation fact, with explicit
reinvocation distinct from loading a saved registration or starting a new turn.
Only trusted host activation can mint or retrieve the binding used by a declaration;
upstream output, arbitrary registration strings and ordinary plan rebuilds cannot
invent an activation or erase a reservation. Source origin determines whether
Claude once is effective (skill frontmatter) or ignored (settings/agent).
The source loader and public management flow will use this same binding later.

- [x] Extend the existing runtime record with bounded durable activation and once
  state. Use the session mutex and the existing clone/validate/publish pattern;
  failed validation must not leak a partial reservation into a later write.
- [x] Integrate eligibility and reservation with existing pre-tool and post-tool
  admission. Preserve unique invocation identity, grouped concurrency, original
  receipts and final-candidate checks. Persist explicit consumption references
  for skipped handlers; never manufacture an invocation outcome or reapply a
  previous rewrite/context effect. Required one-shot behavior must remain explicit
  when a consumed declaration is absent from later execution.
- [x] Settle only the exact reserved invocation. Consume on actual valid success;
  failed/blocked results remain eligible only on a later matching event. Unknown
  effects remain held across restart, task/turn changes, source/generation changes
  and attempted reactivation until explicit reconciliation. Generic recovery
  acknowledgment must not manufacture successful hook evidence.
- [x] Test actual confined command effects, concurrent admission, known failure,
  blocked results, malformed output, cancellation, persistence/restart, duplicate
  references and distinct package/scope/skill identities. Demonstrate an old
  success skips later work without replaying its effects, explicit skill
  reinvocation restores known eligibility, and uncertain attempts cannot escape
  through a new epoch. Cover both pre-tool and post-tool production dispatch.
- [x] Retain reproducible pinned Claude source probes with synthetic local peers,
  executable/input hashes and corrupted-evidence controls. Record the deliberate
  host async difference: Claude consumes on launch, while this contract consumes
  only after actual success. Source probes are not host or live-provider passes.
  Seven source cases and 48 corrupted-evidence controls pass; both independent
  reviews approved [the retained source qualification](../reviews/plugin-once-source.md).
- [x] Run affected tests, Clippy, formatting, independent specification then
  quality review, and the production self-audit before committing this prerequisite.
  Final verification passed 721 tests with 16 explicitly ignored cases, Clippy
  and formatting; both independent reviews approved the candidate. See the
  [runtime review](../reviews/plugin-once-runtime.md) for failure controls and limits.
  Public package activation, remaining lifecycle events and full installed/live
  conformance remain pending; owned async jobs and rewake are verified below.


### Owned asynchronous observers

Decision: [original ownership and allowance](../decisions/keep-asynchronous-hook-work-with-its-original-owner-and-allowance.md).
The [child completion boundary](../decisions/settle-child-observers-before-advancing-supervision.md)
retains the same child session through admitted observer completion and eligible
rewake before supervision advances, without changing the parent phase.
Claude first-line and idle rewake source qualification is independently approved:
seven cases and 135 corruption controls passed, with the seven-case one-shot
regression preserved. See [the source review](../reviews/plugin-async-source.md).
Codex deferred-context qualification also passed specification and quality review:
three cases and 38 corruption controls, with the original three post-tool source
cases preserved. See [the Codex source review](../reviews/plugin-codex-async-source.md).
Owned async execution passed 737 tests with 16 explicitly ignored cases,
all-target Clippy, formatting and independent specification then quality review.
The [runtime review](../reviews/plugin-async-runtime.md) records child-boundary
repairs, fixture corrections, exact evidence and source/live limits.
Remaining lifecycle families and complete conformance are still pending.

Implement after the synchronous one-shot prerequisite is verified. Record the
ownership decision before changing code. Reuse the existing durable hook receipt,
session store, runner supervision and cumulative allocation. Do not add another
job database or redefine an idle foreground task as a cancelled owner.

- [x] Add a bounded observer execution lease for an exact reserved invocation.
  Capture its original task or child, package and policy, allocation, absolute
  deadline and snapshot resources before launch. Retain cancellation and join
  ownership in a bounded runtime collection; jobs must not keep the runtime
  alive through an event sink. Acquiring capacity must precede queueing work.
- [x] Transfer only eligible observer work. Required gates remain synchronous.
  Handle a source-supported first-line async marker while the command is still
  running; waiting for its exit cannot establish asynchronous execution. Keep
  the bounded launch marker separate from final output and reject transfer when
  the admitted role is a required gate.
  Keep the process supervisor, mutation guard and capacity until actual teardown.
  Ordinary turn completion may leave that observer running; explicit cancellation,
  shutdown, owner replacement or expired authority must stop it. Forward idle
  cancellation through both workflow and delegation wrappers.
- [x] Persist transfer and exact completion separately from synchronous lifecycle
  settlement. Launch does not consume a one-shot handler. Actual valid success
  may consume it; uncertain effects remain reserved. Restart restores evidence
  and holds without replaying jobs or pretending the old execution is live.
- [x] Retain bounded attributed context references for delivery at a safe model
  boundary. Late results cannot rewrite an old tool result or authorize a gate.
  Reserve delivery durably; an interrupted delivery cannot be automatically sent
  twice. Command-shaped context is data and cannot invoke developer controls.
- [x] Implement source-supported explicit rewake through internal work admission.
  Ordinary async completion while idle queues context for the next eligible turn.
  Rewake retains the original task or child and remaining allowance; cancellation,
  changed ownership and exhausted corrections prevent it. It cannot allocate a
  new task, borrow another owner's budget or imply acceptance.
- [x] Quiesce admitted writers before verification, review, acceptance and owner
  replacement. Test late completion after a new allocation, weak-owner loss,
  process cleanup, queue saturation, cancellation during UI backpressure and
  restart at transfer, completion and delivery boundaries.
- [x] Qualify configuration and first-line async output, deadlines and rewake
  against the pinned source runtimes. Keep source differences explicit, including
  Claude launch-time once consumption versus the host's actual-success rule.
  Then exercise production effects and downstream requests through native and
  both external loop owners, with independent specification and quality review.


The developer-approved [profile revision 3 correction](../reviews/plugin-profile-revision-3.md)
marks only Codex SessionEnd MCP declarations as source-nonexecuting. Command
shutdown and explicit native MCP execution remain required.

The synchronous tool completion prerequisite is implemented and independently
reviewed. It extends the existing runners and result ledger before the other
lifecycle families below.
The decision [retains original results before lifecycle effects](../decisions/retain-completed-tool-evidence-before-synchronous-lifecycle-effects.md).
External correction [supersedes the old backend turn before replanning](../decisions/supersede-external-backend-turns-before-post-tool-correction.md).
Claude correction [preserves structured replacement content](../decisions/preserve-structured-claude-content-through-post-tool-corrections.md).

- [x] Generalize host-created invocation framing and source result decoding for PostToolUse and PostToolUseFailure without weakening PreToolUse admission. Preserve its existing registration API where practical.
- [x] Persist causal post-operation receipts separately from pre-tool approval, retain original results before awaits, and preserve old serialized records. Distinguish an admitted execution failure from a pre-admission refusal; missing file opens can fail before a mutation starts.
- [x] Use the existing command, prompt, agent, HTTP and MCP runners for every applicable synchronous post-tool source pair. Unsupported source cells remain explicit; they do not acquire another dialect silently.
- [x] Apply bounded attributed context, feedback and supported model-facing replacements while preserving original success and exit status. Honor event-specific continuation and correction semantics with the same owner and allowance; no hook can accept work.
- [x] Enforce continuation holds across native and both external loop owners before further tool/model work, preserving completed effects and visible evidence on failure or cancellation. Do not claim a backend-owned event from a synthetic callback.
- [x] Verify actual write/failure traces, source outputs and downstream requests, malformed/late/cancelled responses, observer failure, duplicate correlation/restart, original allowance, read-only snapshots and no replay. Run affected regressions, then independent specification and quality reviews.

Verification: 283 affected tests, all-target Clippy and formatting passed. The
independent broader Rust run passed 688 tests with 16 explicitly ignored cases.
Specification and quality reviews closed their findings, including typed content,
atomic citation policy, complete frame bounds, pre-reply buffering and dispatch
deadlines. The [review](../reviews/plugin-post-tool-lifecycle.md) retains failure
demonstrations, source qualification and static-check limitations.

Permission events, explicit batches and the remaining real transitions below
are subsequent lifecycle work in this same commitment.
This synchronous prerequisite cannot discharge their conformance obligations.

**Files:** `src/plugins/lifecycle.rs`, `src/native.rs`, `src/session.rs`, `src/workflow/mod.rs`, `src/subagents/`, `src/adapters/`, `tests/plugin_lifecycle.rs`.

- [ ] Add explicit host batch membership/settle barriers and native compaction transactions that retain durable evidence and pinned policy outside model context.
- [ ] Implement admitted workspace changes, dynamic watches, settings/model transitions, setup, notifications, task/worktree/child events and real teammate-idle transitions.
- [ ] Correlate host and backend events once by operation identity; never synthesize an unobserved implicit backend batch.
- [ ] Exercise the complete lifecycle matrix for success, denial, failure, cancellation and resume. Stop corrections consume existing limits and cannot delay cancellation or imply developer acceptance.

## 11. Public management, distribution and presentation

**Files:** `src/plugins/{control.rs,distribution.rs,scaffold.rs,presentation.rs}`, `src/config.rs`, `src/main.rs`, `src/terminal/`, `src/inspection/`, `tests/plugins_terminal.py`.

`session::relay_command` currently rejects workflow controls while a turn runs.
Quarantine needs an authenticated developer control available while a failing
gate is waiting, with cancellation and a durable hold. Do not route it through
the busy-control rejection or through plugin/channel text interpreted as prompts.

- [ ] Add public `plugins`/`skills` controls for install, validate, inspect, enable, disable, reload, quarantine, update, remove, scoped discovery and author scaffolding. These controls use the same production catalog API.
- [ ] Resolve marketplace/remote sources to immutable identities with bounded dependency graphs and no install lifecycle scripts. Activation review includes dependencies and changed access.
- [ ] Expose compatibility at component/field level, missing prerequisites, pinned old references, held gates and recovery choices in bounded responsive views.
- [ ] Implement validated theme/style selection and user-copy editing; preserve original failure evidence and host control instructions. Test narrow terminals, cancellation, restart and source disable.

## 12. Bundled workflow packages

**Files:** `packages/best-practices/`, `packages/cairn/`, `tests/plugin_workflows.py`.

- [ ] Ship readable skills plus explicit executable policy declarations. Best Practices gates consume actual current check/review/todo/allocation records; missing policy is not configured.
- [ ] Implement Cairn's versioned verdict mapping, including explanation-only replies and execution errors. A zero command exit alone never establishes Done.
- [ ] Falsify every executable obligation in ordinary dirty temporary projects. Verify normal application startup without either package enabled.

## 13. Complete evidence and release

**Files:** `scripts/check-skills-plugins-hooks.sh`, `tests/plugin_coverage.py`, pinned fixture manifests and provenance, `README.md`, `.cairn/reviews/skills-plugins-hooks.md`.

- [ ] Build a requirement-to-production-case manifest whose keys cover all 41 IDs, every profile field/branch, every applicability cell and five pinned real packages. Require actual case results, not source-name presence or empty test filters.
- [ ] Run source reconstruction and negative mutation probes, runtime conformance, all four adapter workflows, actual backend barriers and authorized live connector cases. Keep controlled/live labels distinct.
- [ ] Include `tests/plugin_mcp_source_inputs.py --claude <qualified-2.1.267-binary>` in source qualification. Require the pinned executable and a successful actual run; this source check does not replace MCP production tests. Its expected values are retained in `tests/fixtures/plugins/claude-mcp-input-source.json`.
- [ ] Include `tests/plugin_post_source_inputs.py --claude <qualified-2.1.267-binary>` in source qualification. It checks actual SDK tool correlation, success/failure event frames, downstream MCP replacements and interrupted correction with exact user-message acknowledgment against `tests/fixtures/plugins/claude-post-source.json`; it does not establish DemonCoder lifecycle delivery.
- [ ] Include `tests/plugin_codex_post_source.py --codex <qualified-managed-binary>` in source qualification. It checks dynamic-tool correlation, successful post frames, the absence of post events on failed dynamic results, and interrupt acknowledgment plus terminal completion before a correction turn against `tests/fixtures/plugins/codex-post-source.json`.
- [ ] Include `tests/plugin_once_source_inputs.py --claude <qualified-2.1.267-binary> --output <new-output-directory>` in source qualification. Require all seven source cases, their corruption controls and retained artifact hashes against `tests/fixtures/plugins/claude-once-source.json`. Source async launch consumption cannot substitute for DemonCoder's actual-success rule.
- [ ] Include `tests/plugin_async_source_inputs.py --claude <qualified-2.1.267-binary> --output <new-output-directory>` in source qualification. Require first-line transfer, its synchronous control and explicit idle rewake controls, retained raw artifacts and corruption checks. Re-run the one-shot source fixture when its shared peer helper changes. These cases do not establish host async execution.
- [ ] Include `tests/plugin_codex_async_source.py --codex <qualified-managed-binary> --output <new-output-directory>` in source qualification. Require deferred context, its synchronous and failure controls, explicit idle observation, retained raw artifacts and corruption checks. Re-run the original Codex post-tool source cases when their shared helper changes.
- [ ] Include `tests/plugin_non_tool_source_inputs.py --claude <qualified-binary> --output <new-directory>` for actual no-tool SDK submit/Stop frames, source denial and correction, with its corrupted-evidence controls and shared post-tool regression. Source transport success on denial does not satisfy a host gate.
- [ ] Include `tests/plugin_codex_non_tool_source.py --codex <qualified-managed-binary> --output <new-directory>` for actual command-hook submit/Stop frames, source denial and correction, raw identity/correction correlation and corruption controls. Re-run post-tool and async source cases when the shared peer changes.
- [ ] Run formatting, clippy, all existing regression tests, installed language services and the complete plugin gate. Commit implementation before Cairn; commit receipts and captured outputs afterward.
- [ ] Perform specification and quality reviews followed by an adversarial whole-commitment review. Resolve findings as separate implementation actions and rerun affected evidence.
- [ ] Install and verify the final executable. Follow Cairn until Done; only then integrate the feature branch with `git merge --no-ff` and report the complete commitment.

## Requirement coverage

| Requirements | Implementing sections |
|---|---|
| EXT-001, EXT-003, PCOMP-001 | 1, 3, 6, 11, 13 |
| EXT-002, EXT-004 | 6, 11, 13 |
| EXT-005, EXT-006 | 7, 11, 13 |
| EXT-007, PCOMP-003 | 2, 4, 10, 13 |
| EXT-008, PRUN-005 | 12, 13 |
| EXT-009, PCOMP-004 | 3, 8, 13 |
| HOOK-001, HOOK-002, HOOK-003, HOOK-004, HOOK-008, PRUN-001, PRUN-002 | 3, 4, 5, 10, 13 |
| HOOK-005, HOOK-010 | 5, 10, 13 |
| HOOK-006, HOOK-007, HOOK-009, HOOK-011, PCOMP-002 | 3, 5, 10, 13 |
| PLUG-001, PLUG-010 | 7, 10, 13 |
| PLUG-002, PLUG-003, PLUG-006, PLUG-011 | 5, 8, 11, 13 |
| PLUG-004, PLUG-008, PRUN-006 | 9, 11, 13 |
| PLUG-005, PLUG-009 | 1, 6, 11, 13 |
| PLUG-007 | 11, 13 |
| PRUN-003, PRUN-004 | 6, 11, 13 |
