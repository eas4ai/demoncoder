# Skills, plugins and hooks implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development for bounded implementation tasks, with specification and quality reviews. The selected Cairn commitment and its falsifiers remain authoritative.

**Goal:** Deliver every requirement of the selected 41-requirement commitment on all four connections.

**Architecture:** Import packages without executing them, retain immutable generations, and attach one typed plugin runtime to the existing tool and workflow boundaries. Plugin effects use existing confinement, cumulative allocation and durable operation receipts. External backends own their model loops but must await the host's authenticated lifecycle decision before crossing a guarded boundary.

**Tech stack:** Rust, Tokio, Serde, the existing descriptor-based storage and Linux confinement primitives, pinned upstream wire/schema inventories, controlled Python transport fixtures, and installed backend qualification.

## Status and execution rules

- **In progress:** Build and verify the import/validation foundation and backend qualification probes.
- Pending: Integrate lifecycle dispatch and every runner.
- Pending: Integrate state, recovery, services and all package components.
- Pending: Exercise the complete public management and coding workflows.
- Pending: Run full conformance, installed/live cases, adversarial review and Cairn checks.

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

- [ ] Exercise installed Claude's `initialize.hooks` callback registration and a real manual compaction against a local model peer. Capture allow and denial, checking backend state and model requests independently of callback text.
- [ ] Trigger actual automatic compaction and repeat allow/deny. Kill, disconnect and time out the callback path while it is waiting; observe whether the backend crosses the boundary. Keep the owner alive in the disconnected-relay case so the probe does not merely prove killing all processes stops work.
- [ ] Implement the owner-supervised Claude control lifetime and correlated cancellation needed to hold failed barriers. Retest all failure cases through the production adapter.
- [ ] Probe pinned Codex manual and automatic compaction using an isolated hook configuration. If the command runner fails open, patch the exact managed relay boundary in the pinned source. A normal upstream hook must not acquire global behavior changes accidentally.
- [ ] Build and qualify that managed Codex artifact; record source digest, patch digest, toolchain, command and executable digest. Test forged/stale/duplicate acknowledgments and ambient plugin canaries.
- [ ] Register only qualified versions for plugin-dependent execution. Any demonstrated proprietary limitation becomes a concrete Cairn escalation, not a capability warning counted as delivery.

## 3. Runtime wire validation and source semantics

**Files:** `src/plugins/{wire.rs,profile.rs,hook_types.rs}`, `tests/plugin_wire.rs`, runtime coverage data under `tests/fixtures/plugins/`.

- [ ] Compile the frozen JSON schemas with external retrieval disabled. Resolve only the retained schema/reference closure. Validate Claude's nested graph and every union alternative using the recorded event/control roots.
- [ ] Implement the exact 510-cell applicability lookup; missing cells and unsupported source pairs fail validation. Native conversion creates a separate explicit declaration, not a fallback.
- [ ] Implement event-specific command/HTTP/MCP responses and separate prompt/agent schemas. Retain ignored source fields as ignored. Worktree paths, watch updates, elicitation and display responses keep their special meaning.
- [ ] Generate positive and negative shape cases per reachable field/branch and effect cases per event. Remove one nested field handler and one nontrivial response handler in controlled mutation tests; both must fail.

## 4. Final-candidate admission and durable lifecycle

**Files:** `src/plugins/{dispatch.rs,admission.rs,receipts.rs}`, `src/tools.rs`, `src/events.rs`, `src/workflow/{runtime.rs,state.rs,store.rs}`, `tests/plugin_admission.rs`.

- [ ] Persist invocation identity and causal source before effects. Implement transformer, decision, observer and legacy-combined classes. Native priority and source concurrent groups retain their specified ordering.
- [ ] Freeze the candidate after at most four revisions; recompute applicable matchers, retain every deny and reject conflicting concurrent rewrites. A stale combined decision requires a declared read-only endpoint or a visible hold; never rerun its effects.
- [ ] Capture gate read sets, including absent paths, file kinds, symlink targets, modes, ownership and ACLs. Gates inspect an immutable dirty/untracked admitted snapshot. Rescan before release and compare inside the host mutation boundary.
- [ ] Bind developer answers to the frozen candidate and authenticated control origin. Input, configuration or permission changes invalidate pending answers.
- [ ] Test actual write outcomes for both handler orders, delayed stale results, permission revocation, uncertain admissions and cancellation. Preserve original operation receipts before observers or display transformations.

## 5. Five confined runner types

**Files:** `src/plugins/runners/{mod.rs,command.rs,model.rs,http.rs,mcp.rs}`, `src/worktree_access.rs`, `src/workflow/review.rs`, `tests/plugin_runners.rs`.

- [ ] Command: JSON stdin, explicit argv/shell, declared environment/access, bounded output/deadlines and owned descendants. Literal event values never enter executable shell source.
- [ ] Prompt: selected model, no tools, schema-validated verdict, cumulative admission and usage. Agent: immutable admitted snapshot, bounded read-only inspection, retained evidence and the same ledger.
- [ ] HTTP: explicitly bound endpoint/headers/credentials, redirects checked before disclosure, bounded response and cancellation. MCP: admitted service/tool and structured/text response handling, with `isError` as failure.
- [ ] Apply source-specific continuation, `impossible`, once, async and rewake semantics. Reserve once before effects, consume only after success; unknown outcomes remain reserved for reconciliation.
- [ ] Demonstrate private-canary denial, injection resistance, descendant cleanup, malformed responses, timeout holds, late observer outcomes and no hidden/unaccounted model calls.

## 6. Package catalog, activation and versioned state

**Files:** `src/plugins/{catalog.rs,state.rs,activation.rs,recovery.rs}`, `src/workflow/{runtime.rs,state.rs}`, `tests/plugin_activation.rs`.

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

**Files:** `src/plugins/lifecycle.rs`, `src/native.rs`, `src/session.rs`, `src/workflow/mod.rs`, `src/subagents/`, `src/adapters/`, `tests/plugin_lifecycle.rs`.

- [ ] Add explicit host batch membership/settle barriers and native compaction transactions that retain durable evidence and pinned policy outside model context.
- [ ] Implement admitted workspace changes, dynamic watches, settings/model transitions, setup, notifications, task/worktree/child events and real teammate-idle transitions.
- [ ] Correlate host and backend events once by operation identity; never synthesize an unobserved implicit backend batch.
- [ ] Exercise the complete lifecycle matrix for success, denial, failure, cancellation and resume. Stop corrections consume existing limits and cannot delay cancellation or imply developer acceptance.

## 11. Public management, distribution and presentation

**Files:** `src/plugins/{control.rs,distribution.rs,scaffold.rs,presentation.rs}`, `src/config.rs`, `src/main.rs`, `src/terminal/`, `src/inspection/`, `tests/plugins_terminal.py`.

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
