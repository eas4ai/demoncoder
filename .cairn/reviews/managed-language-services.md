# Managed language services review

commitment: managed-language-services
commit: 6590fb9
findings:
  - resolved: LSP-004: Independent filtered filesystem copies close the late private-directory read; corrected startup, late-file, external-root and alias regressions pass.
  - resolved: LSP-003: Directory membership validation detects new public workspace and external dependencies during active queries.
Status: complete

## Initial review before the filtered-view correction

Protocol review examined byte-length framing, UTF-8 splits, malformed headers,
size limits and bounded writes. Integration review found and drove corrections
for cached diagnostics, mixed versioned/unversioned notifications, server exit
status and Rust compiler diagnostics under the socket policy. These corrections
have working tests, but no passing Cairn acceptance receipts yet.

The dedicated working gate completed successfully on 2026-09-09: formatting,
Clippy, all-target tests, all 19 language-service tests including installed Rust
and TypeScript, and the four controlled adapter cycles. Native API adapters used
controlled HTTP responses; Codex and Claude used controlled backend transports.
This is not new live-provider authentication evidence. The broader completed-
product gate has not run for this implementation.

Independent quality review then demonstrated a gap the gate misses. Through
the production OpenAI adapter, initialize a server, create
`nested/.demoncoder/canary` externally, then invoke the adversarial fixture on the
same server. Its retained effects report `protected_read: true` and
`outside_write: false`; unsolicited edits and commands remain rejected. The
normal 17-call adapter cycle otherwise passes. Only synthetic canary bytes were
used. A separate late `.env` probe also read its canary. Source inspection shows
that `.env` filtering in tool arguments is not applied to the server namespace.

Added the unignored production-executor regression
`running_server_cannot_read_new_nested_private_directory`. Running
`cargo test --test language_services running_server_cannot_read_new_nested_private_directory -- --exact --nocapture`
fails with exit 101: `protected_read` is true, expected false. Initialization
completed before the host created the private directory. The corrected case
does not exist yet; the full gate now includes this failing test.

The lifetime defect follows from binding the live workspace and masking only
protected paths present at process launch. Restarting before queries or watching
for new paths leaves background-read and creation races. A workspace copy alone
also leaves live external paths reachable through the broad host read mount.

An enforceable filtered view must include the external read boundary. That can
limit sibling-project dependencies, local tools and live filesystem behavior.
Preserving broad live access instead requires additional filesystem mediation.
This is an access-design choice; do not silently narrow the agreed usability
contract or accept the known disclosure. The current implementation is incomplete.

Ripwire edit-check ran without an arity finding. Its quality delta reported
complexity and duplication increases, and its src-only test map could not map
the integration tests. Neither is recorded as a passing gate. Independent review
found no blocking maintainability issue in the added wrappers or bounded writer;
the confirmed confinement failure remains open.


## Filtered-view implementation review, 2026-09-09

The dedicated working gate now passes formatting, Clippy, all-target tests,
all 30 language-service integration cases including installed Rust and TypeScript,
and the four controlled adapter cycles. New admission tests cover independent
copies, ignored/private inputs and aliases, bounded deny decisions, timer flushing,
overflow hints, policy/deletion validation, and stopping a peer before a blocked
reconciliation scan. These are working checks, not Cairn receipts yet.

The installed save check now requires a real rustc E0382 moved-value error and
its corrected clean result. This stronger falsifier found three issues hidden by
the earlier native type-error test: Cargo needed an anonymous sequenced socket
pair; recoverable server-cancelled requests restarted indexing; and empty pull
reports discarded separately pushed compiler errors. The fixes and controlled
retry/mixed-report regressions pass. Unversioned aggregate reports retain unknown
freshness, and ordinary Bash/worktree socket policies remain unchanged.

Independent specification review found a remaining active-query race by source
inspection. Final validation checks known files, runtime aliases and ignore files,
but not directory membership. Creating a new public module while a request holds
the state mutex cannot be reconciled by the observer until after that request;
its answer can still look available/current. The review's temporary reproduction
harness did not execute due to an incompatible library artifact, so this is not
claimed as an executed failure. Record the finding before adding a production
regression and correcting it as separate implementation work.

The production regression now reproduces the new-dependency race: running
`cargo test --locked --test language_services new_public_dependency_during_navigation -- --nocapture`
failed (exit 101, one failed test). A blocked hover returned an available result
after `new_dependency.rs` was created. The requested source itself was unchanged.
This establishes the negative case before the correction.


The corrected blocked-query regression passes for the workspace and an initially
empty explicitly selected external dependency root. A separate no-event unit
check detects those additions and preserves admission for private/ignored entries.
Independent specification re-review found the correction bounded and nonrecursive,
and reported no remaining source findings. Final quality review and current full
regression gates are still pending before acceptance.

## Independent quality review

The quality review found no blocking production defect. It examined ownership,
cancellation, filtered copies, bounded framing/retries, revision validation,
diagnostic freshness, mutation receipts and actual adapter tests. It accepted
the larger admission/query functions and the incidental enum/bounded-writer
similarities reported by Ripwire; a cross-domain abstraction would add coupling.
Ripwire quality_delta returned exit 2 with complexity/length/duplication findings,
not a passing gate. The src-only test map returned exit 4 and discovered no
integration tests; actual project checks supply that coverage.

Two minor corrections remain before delivery: the manual must say 128 MiB
project plus 4 GiB runtime (the counters are separate), and the unused fd4 launch
branch from the superseded design can be removed to restore ordinary Bash's
original launcher. The existing branch closes fd4 safely; no disclosure was found.
These observations are recorded before applying the small corrections.

The current dedicated working gate passed all 31 integration tests, including
installed Rust/TypeScript and the new-dependency race, all 127 library tests,
the remaining all-target suites, formatting, Clippy and all four adapter cycles.
The two minor quality corrections were then applied and independently re-reviewed:
the README states the separate limits; ordinary Bash's launcher now matches the
committed original exactly. The developer-access suite passed 15 cases (one
explicit external assessment case ignored), and formatting, Clippy and diff
whitespace checks passed after that cleanup. No code-review finding remains.
Cairn receipts, inherited completed-product checks and final commitment review
remain pending; this paragraph is not an acceptance claim.


## Final commitment examination

All six current LSP requirements have passing Cairn receipts from the committed
candidate. The shared mechanism exited zero. Its captured stdout and stderr
hashes match every receipt. It ran formatting, Clippy with warnings denied,
all-target tests, all 31 language-service integration cases including installed
Rust/TypeScript, and the four controlled adapter cycles. The declaration includes
shared runtime, protocol, adapters, tests, scripts, dependencies, specification
and plan inputs. Its registration preflight rejects removed behavioral cases;
its shell exits on a failed constituent before emitting requirement pass lines.

The full `bash scripts/check-completed-product.sh` run also exited zero during
this review. It covered the completed commitments and installed the release
artifact, then exercised installed backend, privacy, generated-output, bounded
review and six verification workflows. This supporting regression output is
retained beside this review; it is not a newly issued AUD-006 Cairn receipt.

- stdout: managed-language-services-completed-product.out
  sha256:f3811c1087a771fd4c6a07bfdd088172a84d5f18becca7444bf318f18743c69b
- stderr: managed-language-services-completed-product.err
  sha256:979c725f99005161111090b5ca9a0b78ec9471fd01d23236205141a09bd9f113
- installed `/home/shawn/.cargo/bin/demoncoder` matches `target/release/demoncoder`:
  sha256:41da299ad980314361cfafed8110b67c790be76301bc4ccd1a6c375c42b8bb9f
  Its actual help includes both server flags and the external read-root flag.

Re-examined the boundaries that simple successful navigation would miss:
startup and late private files, external dependency roots and aliases, policy
revocation during blocked scans, new public directory entries during active
queries, mixed pull/push freshness, recoverable server cancellation, and ownership
of descendants and completed writes. The negative and corrected cases above
establish those failures and repairs. No code changed during this final review.
Only the finished plan checklist and this review/evidence record are updated.

Real language-server checks used installed rust-analyzer and
TypeScript-language-server. The four LSP adapter cycles used controlled model
and backend peers and inspected actual tool receipts. They do not establish new
paid-provider authentication evidence. Empty language diagnostics continue to
mean an observed report, never successful project verification or acceptance.

Production self-audit against rules 1–14: the implementation follows the selected
scope and approved access decision; modules preserve clear ownership; source and
protocol boundaries validate inputs; failures are explicit; private content is
excluded before server admission; disposable state and monotonic revisions handle
replacement and cancellation; memory, copying and execution remain bounded.
The implementation checklist, documentation, decisions, negative tests, installed checks and independent
reviews are complete. The manual states the explicit enablement, dependency and
freshness limits. Ordinary Bash retains its original launch and socket policy.
No unresolved finding or needed production revision remains.


## Final receipt refresh review

Compared every declared input with the earlier reviewed commit. The sole change
is marking the finished implementation-plan checklist complete. Runtime, tests,
mechanisms and specification text are unchanged. Cairn reran the full LSP gate
and issued six current passing receipts; the command exited zero and every
captured stdout/stderr hash was verified. The installed artifact still matches
the recorded SHA256. Re-examined this final difference against the production
rules and the completed review above: no new code, missing verification or
unresolved finding remains. No code changed during this review.


## Review after the extension specification draft

Compared the reviewed candidate with the current tree. Runtime source, tests,
check scripts, dependencies, README and the agreed language-service requirements
and commitment are unchanged. The new extension specification files are marked
Draft; their separate draft commitment is not Current. They preserve the existing
LSP admission, filtered-filesystem and diagnostic-freshness requirements. No new
runtime plugin behavior or broader file access is activated by these documents.

The full managed-language-services mechanism reran against commit 942ef85 and
exited zero. All six requirements have current passing receipts. Verified each
receipt's stdout and stderr digest against the captured files. The run includes
formatting, Clippy, all-target tests, installed Rust/TypeScript language-service
cases and all four controlled adapter cycles. This adds no live-provider claim.

Reviewed the new documents for accidental changes to the agreed LSP contract,
ambiguous Draft status, incomplete requirement coverage and scope reduction.
The draft is one complete extension deliverable with 31 requirements; it does
not count unimplemented components as done. Spec lint, local links, requirement
coverage and staged whitespace checks pass. Ripwire test-gate reported zero
changed symbols. Root quality-delta exited 2 with findings in vendored reference
trees; it is not a passing quality gate or evidence of a production-code change.
No code changed during this review and no new LSP finding remains.

## Review after plugin specification remediation and restart-test correction

Reviewed candidate c9558bf on 2026-09-09. The plugin draft now contains 41
requirements. Its runtime and compatibility contracts address all ten findings
in docs/reviews/skills-plugins-hooks-adversarial.md. The complete feature scope
remains required, and the roadmap still selects this LSP commitment. Draft
contracts and fixed compatibility inventories do not claim implemented plugin
behavior or live backend/connector qualification. Requirement coverage, local
links and anchors, event inventory and specification lint passed.

The first evidence refresh failed two existing tests, and a second run repeated
the version assertion failure. Both runs remain in evidence history. Individual
reruns passed, so those passes were not treated as a resolution. Inspection
traced both failures to valid copied-view retirement: queued filesystem hints
can restart a peer before a query, and the global document counter advances on
reopening. A save-only peer has no diagnostics after restart until it receives
another save. The host must not manufacture a save from a diagnostic query.

Rewriting identical source bytes before the diagnostic query reproduced both
original assertion failures individually: version 2 versus expected 1, and an
explicit pending report. The corrected tests retain this deterministic restart.
They require increasing versions, the SHA256 of current source, current error
counts and unchanged verification state. The save test checks ordinary and
forced-restart queries, accepts only current matching reports or explicit pending,
requires pending after the forced restart, and verifies that neither queries nor
failed edits add a save receipt. This corrects test assumptions; it does not
relax LSP-003 freshness or change production code. Both corrected tests passed
individually before the full mechanism ran.

The full mechanism then exited zero against 4a72dcb and issued six passing
receipts at 20260909T222859657Z/20260909T222859658Z. It includes formatting,
Clippy, all-target tests, all 31 language-service cases with installed Rust and
TypeScript servers, and all four controlled adapter cycles. Verified stdout
and stderr hashes for every receipt. Controlled adapters add no live-provider
authentication evidence.

Compared runtime source, check scripts, dependencies, README and agreed LSP
requirements with the preceding reviewed tree: unchanged. The only executable
diff is the two test corrections above. Inspected their assertions for accidental
acceptance of clean timeouts, stale content, extra saves or swallowed errors;
each remains rejected. Existing private-source and blocked-scan revocation
regressions remain in the passing full mechanism. No production behavior, access
policy or public interface changed.

Ripwire edit-check found no signature change or caller incompatibility. Its
test-gate reported no changed symbols during the document pass. Root
quality-delta exited 2 with vendored reference-tree findings; it is not a passing
quality result. Manual inspection and executed project checks establish this
change's validation. Production rules 1–14 were reviewed: scoped work, explicit
failure history, stronger restart coverage, current documentation and retained
evidence are complete. No unresolved LSP finding remains. No code changed during
this review.

## Review after the second plugin specification remediation

Reviewed candidate 6590fb9 on 2026-09-09 local time. Compared it with 1f40f69:
application source, tests, check scripts, dependencies, README and the agreed
LSP specification/commitment are unchanged. The draft plugin commitment remains
unselected and retains its complete 41-requirement scope.

Examined the three corrections for renewed false-coverage or recovery gaps.
The wire inventory retains nesting, inherited references and alternative branches;
its parser rejects unknown constructs and source drift. Independently reconstructed
types agree with the pinned SDK on ten targeted probes, and nested-field/branch
deletions are detected. Full Codex/portable schemas retain source constraints.
The applicability lookup covers 510 cells; model outcomes distinguish ignored
responses, corrections and stopping with an unmet gate. Missing-cell rejection,
four response-schema checks and fourteen outcome cases pass. These checks assess
the documentary contract, not implemented plugin execution.

The replacement-task recovery keeps one admission owner, fresh instructions and
gate decisions, the same cumulative allocation ledger and settled effect history.
Children, uncertain effects, state preparation and interrupted activation have
explicit outcomes. The old task is superseded rather than accepted; neither
ordinary reload nor an agent message initiates replacement. The original R03
counterexample is resolved without mixing generations in one task.

Specification lint, 41-requirement coverage, local links/anchors and all 316
field pointers pass. All four documentary check commands pass with the recorded
source and parser versions. Scoped Ripwire quality-delta/test-gate on the committed
compatibility directory report no additional uncommitted changes; they are not
evidence that the new specification has an implemented runtime.

The full managed-language-services mechanism exited zero against 7046eff and
issued six passing receipts at 20260910T000904737Z through 20260910T000904739Z.
It ran formatting, Clippy, all-target tests, installed Rust/TypeScript cases and
the four controlled adapter cycles. Every receipt's captured stdout/stderr hash
was verified. No new live-provider or plugin backend-qualification claim is made.

Self-audit against production rules 1–14: the changes stay within the requested
specification remediation, use bounded explicit source inputs, retain reproducible
negative checks and document proof limits. No new LSP finding or unresolved
R01–R03 specification correction remains. Runtime plugin conformance is still
required by the draft commitment. No code changed during this review.
