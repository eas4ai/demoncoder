# Managed language services review

commitment: managed-language-services
commit: c4ff9ad72916d42260d5859667cdd791eee3b75c
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
