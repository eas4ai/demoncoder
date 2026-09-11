# Owned asynchronous hook runtime

Status: this async prerequisite passed final regression, specification review,
quality review and the production self-audit. The complete commitment remains open.

This work implements the [original-owner decision](../decisions/keep-asynchronous-hook-work-with-its-original-owner-and-allowance.md)
and [child completion boundary](../decisions/settle-child-observers-before-advancing-supervision.md)
inside the existing hook ledger, session runtime, command supervisor and task
allocation. It advances HOOK-006, HOOK-008, HOOK-011, PRUN-001 and PCOMP-002.
The full lifecycle matrix, public activation, component workflows and complete
installed/live conformance remain required in the same commitment.

## Implemented behavior

| Boundary | Behavior |
|---|---|
| Admission | Reserve an exact invocation and capacity before launch. Capture the original task or child, required-policy fact, package, policy and absolute deadline. |
| Transfer | Declared async, standalone asyncRewake and supported first-line markers transfer non-required work before exit. Required policy cannot be satisfied by scheduling. |
| Lifetime | Ordinary foreground completion may leave an admitted observer running. Explicit cancellation, shutdown, changed ownership and deadline exhaustion revoke it. |
| Cleanup | Keep supervision, confinement, capacity and mutation guards through teardown. Jobs and event sinks hold weak runtime references. Cleanup timeout leaves work held. |
| Outcome | Retain exact completion separately from original tool settlement. Launch is not one-shot success; uncertain effects remain held. |
| Context | Deliver bounded attributed data at a safe model boundary, reserving delivery durably before sending. Late output cannot change a completed tool or grant developer authority. |
| Parent rewake | Use the original task and remaining correction allowance. Queued developer commands take priority, and cancellation or exhaustion prevents continuation. |
| Child rewake | Keep the same child session through admitted observer completion before supervision advances. Charge its existing correction ledger without changing the parent phase or allocation identity. |
| Evidence | Quiesce admitted writers before verification, review, acceptance and owner replacement. Child checkpoints also wait for admitted observers. |
| Recovery | Preserve historical required-policy and once semantics. Restart holds uncertain execution and delivery rather than replaying them. |

The runtime admits at most eight live observer jobs and 64 pending or reserved
context deliveries. Delivery text is bounded to 64 KiB. A first-line marker is
bounded to 4,096 bytes and uses the existing bounded, cancellation-aware stdout
and stderr reader. A requested timeout can shorten the original deadline.

Ordinary async completion starts no idle model call. Child context remains
available for a later eligible corrective child turn until actual assignment
termination. The child completion boundary drains only already-admitted jobs;
it introduces no new scheduler, database or allowance.

## Source qualifications and deliberate differences

The pinned [Claude source qualification](plugin-async-source.md) passed seven
cases and 135 corruption controls. The pinned [Codex qualification](plugin-codex-async-source.md)
passed three cases and 38 controls. Both passed independent specification and
quality review, with their original shared-helper regressions preserved.
Source qualification was committed as `623b662`.

Claude can deliver context from a command configured to exit 1; that does not
make it a successful host one-shot invocation. Its observed first-line timeout
does not replace the host's finite original deadline. Codex delivers successful
async context on the next explicit turn and omits failed-command context in the
qualified cases. Neither source's model-message role grants host control authority.

These source checks use actual pinned executables with controlled local peers.
They do not establish production host behavior or live-provider conformance.

## Failure demonstrations and repairs

**Child uncertainty.** The actual child writer test first exposed a missing hold:
after a successful late-write control, cancelling the running observer left
uncertain effects, but `Manager::worker_turn` returned success.
`async-child-writer-red-8.log` retains that failure. The repair propagates the
exact child's uncertainty and preserves explicit Cancelled status. The final
three-scenario test checks actual effects, persisted child state and startup
interruption without transferring the hold to the parent.

**Child idle rewake, specification finding S1.** The first implementation advanced
supervision after ordinary foreground completion and later cancelled remaining
child observers. No child idle boundary could admit a delayed rewake using its
remaining correction allowance. The judged child-boundary refinement was
recorded before the repair. `async-s1-child-idle-red.log` demonstrates the old
worker returning success after two foreground responses while its observer
remained paused.

The repair keeps the existing child session active, drains admitted jobs under
the original deadline and cancellation, then reserves eligible rewake from the
exact receipt and supervision ledger before validation. Five scenarios cover
same-child continuation, ordinary async without an idle call, exhausted
corrections, cancellation and changed ownership. The changed-owner test waits
for durable Uncertain state before releasing the observer. Specification
re-review closed S1. The ownership fixture uses a real manager, native session
and tool executor with a paused trusted runner returning raw Claude command
outcomes; actual confined-command transport is covered separately.

**Writer fixture correction.** Initial writer tests did not exercise the intended
effect: bubblewrap could not create the mount destination because an explicit
empty read set omitted it from the snapshot. A directory grant alone did not fix
that. `async-writer-diagnostic-1.log` and `-2.log` retain those failures.
Using the established writer fixture's default read set restored the destination;
`-3.log` passed the actual write before verification. No mount implementation
changed. The final four-scenario test captures file contents and observer status
when verification or acceptance returns, before closing the session. It also
tests close and Cancel/Shutdown with a saturated UI channel.

Earlier child test logs 1–7 failed during fixture setup and are not behavioral
failure evidence. Acceptance without required evidence remains refused; that
scenario is not a successful acceptance result. The focused writer fixture does
not separately establish a successful review/acceptance transition.

## Reviewed candidate and verification

Both reviewers and the parent independently verified Rust SHA-256
`84d58fb3dd30991b620febb30c79e5cbdc97cd7be7432e65155fcfbca3a135f9`
over 182 files. The recipe combines `src/**/*.rs` and `tests/**/*.rs`, sorts
pathlib paths, and hashes each relative path, NUL, file bytes and NUL.
The manifest is `plugin-async-frozen-candidate.json` in
`/home/shawn/demoncoder-check-tmp`, which also holds the logs below.

| Executed check | Result |
|---|---|
| `async-s1-runtime-focused.log` | 22 passed, zero failed; includes migration, once settlement, capacity, weak ownership, recovery, and both child repairs. |
| `async-s1-integration-focused.log` | Six passed, zero failed; actual confined commands, first-line/declared transfer, native and external delivery, parent rewake, deadlines and writer boundaries. |
| `async-once-fixture-clippy.log` | All-target Clippy with locked dependencies and warnings denied passed. |
| `async-final3-fmt.log` and `async-final3-diff.log` | Formatting and whitespace checks passed. |
| `async-final-all-targets-3.log` | All 42 suites passed: 737 tests passed, zero failed, 16 explicitly ignored. |

The integration functions include pre/post × declared/first-line × exit 0/1
combinations, required-gate refusal, standalone asyncRewake without a first-line
marker, shortened first-line deadline, close/cancel and both production external
adapters against controlled protocol peers. Those peers are not installed or
live-provider runs. Existing
`plugin_command_runners::bounded_input_timeout_and_output_flood_retain_failure_diagnostics`
covers the same bounded reader, both streams and their combined cap; no new
first-line flood scenario is claimed. Existing descendant cleanup tests remain
part of the required broad regression.

Independent reports are `plugin-async-runtime-spec-review.md` and
`plugin-async-runtime-quality-review.md` in the same temporary directory.
Specification approval followed the S1 repair, then quality approval followed.
Reviewers inspected source, tests and retained logs and recomputed hashes; they
did not run Cargo. No remaining actionable review finding was established.

## Static assessment

`async-s1-quality-delta.log` returned 2: 232 reported regressions, including
147 preexisting-worse, 85 new-symbol and 72 gating findings.
`async-s1-test-gate.log` returned 4 with 34 mapped test files and 247 symbols
without mapped coverage. Neither result is a pass.

Quality review examined the real complexity increases in native/external turn
loops and command dispatch. Durable ownership and delivery are isolated in
focused modules; the remaining call-site branches did not establish a blocking
refactor requirement. The large existing loops remain a maintenance consideration.

The reviewer traced named trait, Drop, Serde and test methods to actual dispatch.
Small repeated ledger traversals and identity structures have different borrowing
or lifetime rules; directly reusing the service owner check would incorrectly
cancel observers at ordinary foreground completion. No suppression or
acknowledgment was added. The static coverage map is incomplete, not proof of
247 untested behaviors. `async-s1-edit-check.log` returned 0 for `worker_turn`,
with four callers and no detected incompatible use; its counts are lower bounds.

## Broad regression corrections and final disposition

The first broad run, `async-final-all-targets.log`, stopped in the library suite
with 249 passes and three failures. Synthetic captured-plan declarations omitted
the newly explicit required-policy fact. The first focused correction exposed
two similar post-tool entries; `async-admission-fixture-green.log` records nine
passes and one failure despite its filename. Adding the existing policy facts
to all three entries preserved production comparisons and test assertions.
All ten admission tests then passed in `async-admission-fixture-green-2.log`.

The second broad run, `async-final-all-targets-2.log`, passed all 252 library
tests, then failed one of 19 one-shot tests. The command fixture changed its
post-hook class to Observer but inherited required policy from its generic
Combined declaration. The fixture now explicitly retains required pre-tool and
non-required post-observer policy. Production continuation and uncertain-delivery
handling were unchanged. All 19 one-shot tests passed in
`async-once-fixture-green.log`.

For each correction, the parent and reviewers verified the new hash and reversed
only the fixture edits in memory to reproduce the previously approved hash.
Both reviewers confirmed approval after each narrow change. The failures remain
preserved; no production gate or existing assertion was weakened to obtain passes.

The final command was `cargo test --locked --all-targets --no-fail-fast`.
It exited zero with 737 passes, zero failures and 16 explicitly ignored cases
across 42 suites. The parent independently parsed all suite results and verified
the unchanged 182-file hash after execution. Current-candidate Clippy, formatting
and diff checks passed. The retained summary is
`async-final-all-targets-3-summary.json` in the temporary evidence directory.

No remaining actionable finding was established in this prerequisite. These
development checks are not Cairn receipts or full installed/live conformance.
The complete lifecycle matrix, package activation and component workflows remain
required before the commitment can be Done.

## Production self-audit

| Rule | Assessment |
|---|---|
| 1. Understand | Used the agreed owner/lifecycle contracts, actual source qualifications and existing runtime boundaries. |
| 2. Coherent change | Extended existing receipts, command supervision, session loops and child manager; no second durable job system or allowance. |
| 3. Maintainability | Isolated ownership, delivery and legacy decoding; quality review assessed real complexity and justified small duplicated structures. |
| 4. Boundaries | Exact owner, declaration, required policy and delivery identities remain checked; legacy Observer and Combined/gate semantics are preserved. |
| 5. Errors and secrets | Failures retain original effects and actionable holds; command execution retains existing confined environment and credential boundaries. |
| 6. Security | First-line output cannot downgrade required policy; attributed context bypasses developer-command parsing and cannot grant authority. |
| 7. Survivable state | Interrupted execution and reserved delivery remain held after restart; once consumption requires actual valid success. |
| 8. Reliability | Execution, delivery, deadlines and cleanup are bounded; weak ownership and actual effect tests cover cancellation and writer quiescence. |
| 9. Tracking | This prerequisite is verified; lifecycle dispatch remains the single active plan item, with full components and conformance pending. |
| 10. Verification | Final 737-test broad run, Clippy, formatting and independent reviews passed after the preserved failure demonstrations and fixture corrections. |
| 11. Honest reporting | Retained failed runs, fixture-only failures, controlled peer limits, 16 ignored tests and static nonpasses. |
| 12. Partnership | Recorded the child-boundary choice before repair and kept the original full commitment. |
| 13. Release gate | Independent findings are closed, broad regression passed, and frozen inputs were verified; no known prerequisite defect remains. |
| 14. Clear writing | Separated implemented behavior, failure demonstrations, source differences, verification and remaining work. |
