# Verification, review, and recovery implementation plan

> Execute inline with superpowers:executing-plans. Cairn remains the referee;
> each failing requirement is repaired and verified before acceptance.

**Goal:** Deliver the agreed VERIFY-001 through VERIFY-006 workflow in the real terminal.

**Architecture:** The current session owns task state and adapter execution.
Native adapters expose serializable conversation checkpoints. A private durable
record guards admissions and stores original outcomes. Existing ToolExecutor
runs checks and preserves access and cancellation behavior.

**Tech stack:** Rust, Tokio, Serde, current terminal and provider adapters;
SHA-256 for workspace identity and existing private-file primitives.

## File responsibilities

- `src/workflow/mod.rs`: developer commands and task lifecycle, delegates to Session.
- `src/workflow/state.rs`: acceptance, evidence, findings and correction state.
- `src/workflow/workspace.rs`: bounded workspace capture and identity, no mutations.
- `src/workflow/store.rs`: private versioned durable state with one writer.
- `src/workflow/allocation.rs`: cumulative deadlines and admission counters.
- `src/workflow/review.rs`: evidence-bound, tool-free review and verdict validation.
- `src/session.rs`, `src/native.rs`: owner integration and native checkpoints.
- `src/events.rs`: original event retention and allocation observation.
- `src/config.rs`, `src/main.rs`, `src/terminal.rs`: explicit configuration,
  restoration selection and visible workflow state.
- `src/adapters/{openai,anthropic,codex,claude}.rs`: checkpoint/capability
  declarations and actual admission boundaries, preserving loop ownership.
- `tests/verification_workflow.rs`, `tests/verification_workflow.py`: state
  adversaries and real terminal/provider/subprocess exercises.
- `scripts/check-verification-review-recovery.sh`: per-requirement evidence.

## Task 1: acceptance state and actual workspace identity (VERIFY-001)

- [x] Add an initially failing terminal case: submit `/task change greeting`,
  attempt `/accept` with no checks, and require a visible unverified refusal.
  Run `python3 tests/verification_workflow.py --requirement VERIFY-001` against
  the built baseline and retain the observed failure.
- [x] Add state tests for failed, absent and stale evidence. The essential
  invariant is `assert!(task.accept(&current_snapshot).is_err())` until all
  required check receipts and a clear review name that exact snapshot.
- [x] Implement bounded workspace capture including untracked and pre-existing
  files, and explicit acceptance transitions. Never invoke repository hooks
  or filters while collecting evidence; never modify developer files.
- [x] Integrate developer workflow commands through the current Session owner;
  keep ordinary conversation completion distinct from acceptance.
- [x] Commit the implementation with the focused state, terminal, format and lint checks passing (this commit).

## Task 2: checks and reviewer evidence (VERIFY-002, VERIFY-003)

- [x] Add PTY cases that execute `test "$(cat greeting)" = corrected`, first
  failing and then passing after a real worker edit. Assert original failure,
  command identity and workspace identity remain in retained evidence.
- [x] Test cancellation with a child that writes a disposable marker repeatedly;
  require no further writes after two seconds. Exercise outside-write and
  synthetic credential canaries through the shared executor.
- [x] Capture reviewer HTTP input and assert it contains actual source/patch,
  task requirements and failing/passing check evidence, with no tools.
- [x] Reject review tool requests, malformed JSON, missing evidence, excessive
  output and changed workspaces. Each negative case must prevent acceptance.
- [x] Implement execution and review using the existing adapters and ToolExecutor;
  run targeted cases. Commit before Cairn checks below.

## Task 3: correction and cumulative allocation (VERIFY-004, VERIFY-005)

- [x] Drive worker -> failing check -> reviewer finding -> `/correct` ->
  passing checks -> fresh review -> explicit `/accept` through the real app.
- [x] Set two correction rounds, repeatedly return a finding, and assert the
  third correction performs no provider request and retains findings.
- [x] Set tiny call/tool/deadline allowances and cross worker/check/review and
  Oracle boundaries. Assert admissions stop before effects after exhaustion.
- [x] Keep unavailable token/cost fields unknown; refuse hard limits the chosen
  adapter cannot enforce. Check ordinary follow-up cannot reset task counters.
- [x] Implement the shared admission owner and run these cases with format/lint checks.

## Task 4: recovery and retained decisions (VERIFY-006)

- [x] Kill the application during a controlled mutation, then resume its private
  record. Assert the marker is not repeated and the operation is uncertain.
- [x] Restore known completed native conversation/tool results, findings,
  acceptance decisions and cumulative allocations without replaying effects.
- [x] Test crash positions before admission, during execution and after durable
  completion. Exercise truncated/corrupt files, concurrent open, write failure,
  changed workspace and unsupported external-backend restoration.
- [x] Implement versioned durable transitions and checkpoint restoration. Require
  an explicit reconciliation explanation before continuing uncertain work.
- [x] Run the production restart cases and targeted storage tests.

## Task 5: commitment acceptance

- [ ] Run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
  `cargo test --locked --all-targets`, required terminal mechanisms and release build.
- [ ] Run each Cairn-named check against committed inputs; commit every receipt.
- [ ] Review the complete change for correctness, permission/cancellation gaps,
  lost evidence, stale acceptance and unsafe recovery. Record findings before fixes.
- [ ] Install and exercise the installed binary through the controlled workflow.
- [ ] Record the 14-rule production self-audit and limitations; require Cairn Done.

The four implementation tasks cover every agreed requirement. Unit checks alone
cannot complete any requirement whose falsifier names the actual terminal.

## Implementation verification notes

The initial production terminal test failed because no separate task acceptance
state existed. Current state and terminal cases exercise absent, failed and stale
evidence, contradictory review, bounded correction and explicit acceptance.
Storage reviews found and corrected unreadable nested records; supervisor reviews
found and corrected detached descendants, an uncancellable handshake and a deep
fork-chain cleanup delay. The 230-level fork-chain regression failed before its
fix; independent inspection measured 231 descendants stopping in 0.214 seconds
after the fix. Fixture failure cleanup retains pidfds before stopping owners.

The correction regression failed when interrupted reverification allowed ordinary
work with zero correction rounds. The corrected gate checks retained workflow
history. Full review history now preserves current findings before refusing work.
Recovery checks cover both native adapters at pre-execution, mid-mutation and
post-result crash points, plus corrupt records, exclusive opens, write failure,
changed workspaces, fixed connection authority and unsupported backend recovery.
Archived tasks retain their original allocations and usage. Clock regressions
cover durable rollback holds without discarding completed results.

The local six-requirement driver passed before committed evidence collection.
Broad Rust tests passed (126 tests, six explicit-driver entry points ignored);
subsequent focused runtime tests also passed. Formatting and Clippy checks passed.
Production checks covered all four ordinary connections, cancellation and
continuation after inspection, queues, access, configuration, status and terminal
interaction. Screen-test races were corrected to wait for actual output and page
redraw. Specification lint passed after sentence-only clarification.

The full coding/connection scripts reached retained live evidence checks, which
require committed inputs. Those retained provider/Oracle records are not fresh
evidence for this change. The new workflow checks use controlled local peers and
make no paid provider calls. Record final committed checks and the release audit
in Cairn's commitment review.

Ripwire was run with reference and build trees excluded. Its name-based graph
reports trait methods and test entry points as untested/dead despite executed
coverage; its test-gate lists obligations rather than recording test results.
Quality delta also reports larger event/config declarations, dispatcher branches,
recent churn and similar Oracle/reviewer protocol handling. Native interruption
handling was consolidated. Remaining protocol loops keep their distinct limits,
usage events and verdict contracts; extracting a generic loop here would obscure
those boundaries. Component spec and quality reviews examined these paths.
