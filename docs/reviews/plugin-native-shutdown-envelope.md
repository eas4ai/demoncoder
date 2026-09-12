# Native application shutdown deadline integration

Status: scoped implementation and verification complete. Fresh specification and
quality reviews pass. The lifecycle commitment remains incomplete.

## Finding

After the lifetime prerequisite passed its session-driver tests and reviews,
inspection of the actual application entrypoint found a three-second timeout
around sending Shutdown and awaiting the session worker. It can abort the worker
before the newly admitted five-second native end-observation and cleanup budget
finishes, or before ordinary resource cleanup after that observation. The earlier
raw tests remain valid for their tested session-driver boundary; they did not
exercise this outer application deadline. A production-path failing control and
corrected control are required here.

## Chosen correction

Reserve the existing three seconds for ordinary shutdown plus the native
five-second observation budget when the session opens a native lifetime. Keep
external-only sessions at three seconds. Use the same native bound internally and
in the outer reservation, keep queue submission inside the outer timeout, and
preserve abort-and-join timeout handling. Do not grant new hook or model authority.

The application captures the original native lifetime before moving the session
into the worker. Workflow and delegation wrappers forward that capability, so a
later replacement with an external session retains the reservation. The extracted
`session::shutdown` is the function called by both main and the regression tests.
Its outer error represents timeout; its inner error represents worker or join
failure. This preserves main's existing error priority: timeout wins immediately;
otherwise a UI error wins over a worker error.

## Executed evidence and independent review

Raw development logs and reviews are retained under
`/home/shawn/demoncoder-check-tmp/`, with prefix `native-shutdown-envelope-`.
These are working-tree checks, not committed-tree Cairn evidence receipts.

| Evidence | Actual result |
|---|---|
| `red.log` | The real confined end command plus delayed ordinary close fails with shutdown timeout: exit 101, 0 passed, 1 failed, 3.24 seconds. |
| `green.log` | The same regression passes after the correction: exit 0, 1 passed, 3.79 seconds. |
| `focused.log` | Both application shutdown cases pass: 2 passed in 7.24 seconds, including native-to-external replacement. |
| `queue.log` | Both queue/error tests pass in 8.00 seconds. Four cases cover native/external budgets, full queues and stuck workers; task guards are dropped before the helper returns. The printed panic is intentional and asserted. |
| `inner-cap.log` | The real end command and detached descendant cleanup test passes in 2.34 seconds through the application helper, with no subsequent writes and elapsed time below 5.3 seconds. |
| `all-targets.log` | `cargo test --all-targets`: exit 0; 44 suites, 873 passed, 0 failed, 17 ignored; summed suite time 595.35 seconds. |
| `output-limits.log` | Separate Python application/PTY checks: exit 0, 3 passed in 4.018 seconds. Cargo does not run this script. |
| `fmt.log`, `clippy.log` | Formatting and `cargo clippy --all-targets -- -D warnings` both exit 0. |
| `spec-review.md`, `quality-review.md` | Fresh independent reviews pass after reading the actual implementation and raw evidence. |

The full regression log is 90,587 bytes with SHA-256
`07e2ec96498cd644faa5b24e86d8a5c5d73cc35e47943adba2f17a8679ab4ecb`.
The parent and quality reviewer independently parsed its results. The implementer
recovered the original command's exit 0 after the usage interruption. All five
changed source/test hashes still match the candidate captured before review and
final checks. The parent also ran `git diff --check` successfully.

The RED source was not separately retained. The implementer's reconstructed
account says the old main sequence was extracted unchanged into the helper, with
an ignored native flag and the literal three-second timeout. The captured command
was `cargo test --test plugin_command_runners application_shutdown_reserves_native_end_and_ordinary_close_time -- --exact`.
The retained failure and timing agree with that account, but do not authenticate
the RED source. The corrected test exercises a real one-second confined command
and 2.5-second delayed ordinary close, asserts the command effect and retained end
receipt, and requires successful completion after three and before eight seconds.

## What the reviews challenged

Both reviewers checked original capability capture through the production
wrappers, replacement behavior, queue backpressure, abort completion, shared inner
and outer bounds, and error propagation. They found no required revision. The
parent's late integration finding is resolved by a test of the function main uses.
The overall lifecycle item remains the sole active plan item.

Ripwire `edit-check` exits 0. `quality-delta` exits 2 and is not a pass: all 14
findings were examined. Ten dead-code rows are actual test entry points, a
constructed fixture and trait methods reached through session dispatch. The
81-line replacement test keeps one coherent setup/action/assertion scenario.
Three gating churn rows describe main's helper integration, the equal inner-bound
constant substitution, and wiring the existing descendant test through the
production helper. No structural deterioration or unrelated edits were found.
No baseline or suppression was added.

Ripwire `test-gate` exits 4 and is not a test execution result. It names 37 test
files and 260 impacted symbols, displaying only the first 25 unmapped symbols.
The broad families include adapters, admission, lifecycle observers, terminal
flow, subagents, language services, tool operations and command cleanup. The Rust
suite runs the normal targets; it does not establish coverage for every unmapped
symbol. Live Oracle and installed-backend Python launchers and ignored cases are
separate qualification requirements. They were not rerun or claimed passing for
this repair, and remain required for the wider commitment.

## Production self-audit

1. Actual main, wrappers, end observation and resource close were traced before editing.
2. The production change is one extracted helper and one shared bound, with focused tests.
3. The session layer owns shutdown timing; the nested result has a documented purpose.
4. External timing, inner native timing and error precedence retain their contracts; no schema changes.
5. Original errors remain intact and no new credential or sensitive output is introduced.
6. More cleanup time grants no command, model, tool or network authority.
7. The initial lifetime survives replacement; timed-out workers are aborted and joined.
8. Queue submission and join have one deadline; actual descendant cleanup is exercised.
9. The plan retains exactly one active item and this prerequisite is only marked verified now.
10. Focused, full Rust, PTY, formatting and lint checks passed; separate live checks are disclosed.
11. Static nonzero results, RED source limits and incomplete commitment status are explicit.
12. The repair stays within the agreed lifecycle work and introduces no speculative cleanup.
13. The implementation and independent reviews require no further revision for this repair.
14. The decision, comments and report explain the concrete timing change in plain English.

## Limits and remaining work

The descendant test proves real cleanup below the inner cap; it does not exhaust
the entire five-second reserve. UI error priority is checked by source equivalence,
not a new terminal-error test. Tokio abort remains cooperative and cannot interrupt
arbitrary synchronous blocking code. The fixed outer envelope includes queue wait;
it does not promise unbounded resource cleanup. Explicit session-hook allowances,
remaining lifecycle events and all final package/backend conformance are still open.
