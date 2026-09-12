# Durable synchronous one-shot hook review

Status: implemented, verified and independently approved for this prerequisite.
The full skills, plugins and hooks commitment remains in progress.

## Implemented contract

The existing durable session record owns activation and one-shot evidence.
Explicit skill invocation creates an activation epoch; rebuilding a plan or
starting an ordinary turn reuses it. Package and service generation remain
separate. Claude settings and agent frontmatter ignore `once`. Codex has no
source one-shot field and cannot mint this binding. Native and Claude skill
declarations use an explicit host-created binding.

A binding identifies the captured canonical package source as well as its
manifest name. Aliases of one root share identity; different roots with the same
name remain distinct. Code or directory replacement at the same source cannot
remove an unresolved attempt. Every package runner constructor validates source
identity, including when a later declaration removes `once`.

Eligibility and reservation share one transaction before preparation or execution.
Known successful completion consumes the handler for that activation. Known
failure or blocking leaves it eligible on a later matching event, never a retry
of the same operation. Unknown effects remain held across restart, generation
changes and attempted reinvocation. Failed transactions cannot publish partial
reservations through a later update.

A skipped handler references its earlier successful invocation. It does not
manufacture an execution, reapply context or rewrites, or supply a fresh decision
about another candidate. Final-candidate access and freshness checks still apply.
Post-tool handling retains original effect evidence before settling consumption.

Exact recovery retains the developer's attestation of an unsuccessful attempt
separately from its original unknown result. It cannot certify observed success,
approve work or replay the old operation. Generic recovery acknowledgment cannot
supply this attestation. Recovery must wait for both active lifecycle settlement
and runner teardown to end.

## Failure demonstrations and resolved findings

- Source identity: runner constructors initially omitted package-source binding
  in HTTP, MCP and model paths. Constructor regressions failed before repair and
  passed after it. MCP reuse also checks the actual managed service's canonical
  source root, alongside its existing complete identity.
- S1, preparation ordering: four production cases showed preparation counters
  and actual MCP initialization requests equal to one when an old unknown attempt
  required zero, in both dispatch paths. Every runnable handler now reserves
  before preparation, even when `once` is omitted. Those four cases and the
  consumed-skip control passed. Ordinary startup controls also executed.
- MCP test expectations: reservation-first ordering invalidated four old
  empty-ledger assertions. They now require one uncertain reservation with no
  outcome, while retaining their independent no-call, no-write, cancellation and
  no-replay checks. A cleanup assertion had caused a second panic; cleanup still
  joins its peer and an ordinary peer failure still fails the test. The full
  25-case MCP suite passed. Earlier failed runs remain failed evidence.
- Q1, settlement ownership: a production two-group post dispatch accepted failure
  attestation while the later group still owned pending first-group context.
  The failing interleaving passed after a separate weak lifecycle token was
  retained through all groups and final proposal settlement. Registration and
  attestation use the same runtime-to-owner lock order. The token does not hold
  runner capacity; the bounded registry prunes dead owners. The companion test
  confirms that abandonment and store reopen still permit exact failed
  attestation without changing raw outcome bytes or applying pending context.
- Runner cleanup: a confined command test pauses its owned supervisor with a
  pidfd and checks that reconciliation refuses while cleanup still owns the
  runner lease. Guarded continuation and finite waits then permit teardown and
  later eligibility. This remains separate from lifecycle completion ownership.

The broader tests cover real confined effects, concurrent and duplicate admission,
known failure and malformed responses, cancellation, persistence, foreign bindings,
source replacement, new epochs, exact recovery and original evidence retention.

## Verification and independent review

Frozen Rust candidate: `7be6c0ad2749026841a06abcf7c3c2f6d94cbce741445e9942b40399c917b9ca`
across 174 files. Hash sorted `Path` values under `src/**/*.rs` and `tests/**/*.rs`,
feeding each relative path, NUL, file bytes and NUL to SHA-256. Both reviewers and
the parent independently matched it; it remained unchanged throughout final checks.

- `cargo test --locked --all-targets`: 721 passed, zero failed, 16 explicitly
  ignored, across 42 suites; exit zero. Log:
  `/home/shawn/demoncoder-check-tmp/once-final-all-targets.log`.
- `cargo clippy --locked --all-targets -- -D warnings`, `cargo fmt --check` and
  `git diff --check`: exit zero. Clippy and formatting logs use the same directory
  with names `once-final-clippy.log` and `once-final-fmt.log`.
- Independent specification review approved the repaired candidate, followed by
  independent quality approval. Reports are `plugin-once-runtime-spec-review.md`
  and `plugin-once-runtime-quality-review.md` in that directory. No remaining
  blocking finding was established for this prerequisite.

Ripwire is not reported as passing: final quality-delta exited 2 with 172 reported
regressions and 46 gating flags; test-gate exited 4 with 255 symbols lacking a
mapped test. Review assessed the real dispatcher complexity, repeated fixture
helpers and unchanged-symbol attribution. New owner and dispatched test types
were classified as dead despite their direct uses and executed regressions.
Static mapping gaps do not establish absent behavior coverage, and executed
checks do not turn the static reports into passes. No suppression was added.
Outputs are `once-final-quality-delta.txt` and `once-final-test-gate.txt`.

The independently approved [pinned source fixture](plugin-once-source.md) records
seven source cases and 48 corrupted-evidence controls. Those observations do not
constitute host or live-provider evidence. Ignored Rust cases remain unverified
by this run; no installed/live or complete-commitment pass is claimed.

## Production self-audit

| Rule | Assessment for this prerequisite |
|---|---|
| 1. Understand before editing | Mapped the durable ledger, source binding, admission, runner and post-settlement boundaries against the selected contract. |
| 2. Small coherent change | Extended existing receipts and ownership; added no parallel persistence or transport stack. |
| 3. Maintainable code | Kept durable state in the runtime and source binding in plugin types; removed duplicated preparation ordering. Quality review approved the final structure. |
| 4. Boundary contracts | Opaque host bindings, constructor checks and retained old-record decoding are covered with callers and fixtures updated together. |
| 5. Errors and secrets | Unknown effects stay visible and held. Source probes use isolated synthetic peers and credentials. |
| 6. Security | Canonical source checks, existing confinement and final-candidate authority remain enforced; recovery cannot grant success or approval. |
| 7. Survivable state | Tested atomic reservation, later-event eligibility, exact recovery, cancellation, restart and concurrent settlement. |
| 8. Reliability | Bounds, weak ownership, consistent lock order and actual cleanup remain enforced; paused-runner and paused-group tests challenge release timing. |
| 9. Todo tracking | Only complete lifecycle dispatch remains in progress; prerequisite checks are complete after code and verification. |
| 10. Verification | Final full Rust tests, lint and formatting passed; meaningful failing controls preceded both review repairs. Static and ignored-case limits remain explicit. |
| 11. Honest status | This is a verified synchronous prerequisite, with no full-product or live-provider completion claim. |
| 12. Technical partnership | Recorded the implementation decision and resolved review defects within the agreed full scope. |
| 13. Release self-audit | No remaining finding requires revision in this reviewed prerequisite. Full delivery still requires the pending work below. |
| 14. Clear language | Decision, plan and review identify concrete behavior, evidence and limits. |

Public activation and recovery controls, owned asynchronous jobs, the remaining
lifecycle events, all package components and full installed/live conformance
remain required in this same commitment. These development checks are not Cairn
requirement receipts; final committed-tree mechanisms still must execute.
