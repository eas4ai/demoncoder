# Native failed-turn observation

Status: implemented and verified, with independent specification and quality
reviews passed. This is a prerequisite of skills-plugins-hooks, not completion
of the commitment.

## Observed missing behavior

An actual returned native provider failure ended its turn without dispatching
StopFailure. The retained boundary failure test observed zero handlers where one
was required. The observation must happen after model admission settles and before
its original durable turn ends. It cannot retry the task or replace the error.

Testing actual MCP observer execution also found that its runner validated all
responses as required gates. The correction uses the admitted invocation role;
required gates retain their existing validation.

Cancellation testing found that advisory errors were displayed without being
retained in the session record. Bounded diagnostics belong to the original native
turn and must remain visible through inspection after reopening. Recording a
cleanup diagnostic does not authorize another operation.

Fresh specification review found a further provenance defect: OpenAI and
Anthropic adapters emit context before sending their HTTP request. A closed
terminal receiver can therefore make `Model::response` fail without any provider
request. Event-log persistence can also fail after a provider response. Treating
every error from that method as a provider failure is unsound. This finding
was resolved with an actual adapter failing control and a verified fix.
The actual OpenAI pre-request control reproduced the defect: after checking that
its listener received no connection, it observed one failure handler instead of
zero. The failing output is retained in
`/home/shawn/demoncoder-check-tmp/native-stop-failure-provider-origin-red.log`.
The corrected pre-request control passed for OpenAI and Anthropic; its raw output
is `native-stop-failure-provider-origin-green.log` in the same directory.
The final corrected focused run passed 25 tests across five suites, independently
counted from `native-stop-failure-focused-builder-final.log`. It includes local
configuration, pre-request terminal failure, post-response terminal and durable
publication failures, and actual transport, protocol and metadata failures through
production adapters with local peers. Marker guidance retains the original cause.
Independent specification re-review passed after the request-builder correction.
Final full regression checks and quality review passed.

Re-review found that an invalid credential header can make reqwest request
construction fail before transport. Wrapping the complete shared HTTP helper
still mislabeled that local configuration error as a provider failure. The
correction must finish request construction before marking transport/status
errors. Specification re-review confirmed this narrower finding is resolved.
The actual OpenAI invalid-header control reproduced one observer where zero was
required, with no connection to its listener. Its failing raw output is retained
as `native-stop-failure-request-builder-red.log`. Typed reqwest builder errors
from execution also stay local, covering invalid URIs rejected after construction.

## Verification

The checks exercise actual native failure and each runner, invalid observer decisions,
malformed responses, model accounting, one-shot ownership, cancellation and queued
and in-flight shutdown. They cover unknown categories and current-turn text, stale
handles, expired ownership, backward-compatible records, and no delayed context or
rewake. They verify that hook and cleanup errors cannot replace the provider error.
Affected regressions, formatting, Clippy, and fresh specification then quality
reviews have run. Final results follow.

The pre-provenance-fix all-targets run exited successfully. RTK reported 833
passing tests and 17 ignored across 44 suites in 535.76 seconds. Its summary is
retained at `/home/shawn/demoncoder-check-tmp/native-stop-failure-cargo-all-targets.log`.
This historical result does not close the provenance finding or verify its fix.
The raw log initially supplied for that run belonged to an earlier failing test
setup. Its body showed 329 passes and one invalid-budget fixture failure, so it
was relabeled `native-stop-failure-initial-invalid-test-budget-failure.log`.
The successful historical run has summary-only evidence. The final run captures
raw output directly, without relying on the latest RTK failure log.

The final raw all-targets run passed 841 tests with zero failures and 17 ignored
across 44 suites (515.56 summed suite seconds). The parent independently parsed
every suite result in `native-stop-failure-all-targets-final-raw.log`; its SHA-256
is `84e23cd9468231f1bded6cc8ba410087724bdaca51af4424826504090eff45e9`.
The parsed summary is `native-stop-failure-parent-final-regression-summary.json`.
Clippy with warnings denied and `cargo fmt --all -- --check` also passed, with
raw logs `native-stop-failure-clippy-final.log` and
`native-stop-failure-cargo-fmt-final.log` in the same directory. Ignored tests are
not counted as passes. These are development checks, not Cairn receipts.

## Static diagnostics

`git diff --check` and the qualified `lifecycle_report` edit check passed on the
stable candidate. Ripwire quality-delta exited 2 with 119 rows, including 37 gating
findings. Its test gate exited 4 with 24 changed symbols, 851 impacted symbols,
39 test entries and 406 symbols without a mapped test. These are diagnostics, not
passing checks or a claim that every suggested test ran.

The retained logs are
`/home/shawn/demoncoder-check-tmp/native-stop-failure-quality-delta.log` and
`/home/shawn/demoncoder-check-tmp/native-stop-failure-test-gate.log`.
The report's added loop uses the existing quoting helper to expose bounded
persisted turn diagnostics. Final review dispositions follow.

After the provider-origin correction, the repeated quality delta exited 2 with
140 rows and 43 gating findings. The test gate exited 4 with 26 changed symbols;
its other counts stayed the same. The corrected-candidate logs use the
`native-stop-failure-final-` prefix in the same scratch directory. These reflect
the intermediate provider-origin correction; neither command passed.

The final request-builder correction produced 143 quality rows with 43 gating
findings (exit 2). The final test gate reported 27 changed and 863 impacted symbols,
39 test entries and 418 symbols without mapped tests (exit 4). Retained files use
`native-stop-failure-builder-` prefixes.

Quality review examined the 43 gating rows: 16 recent-churn rows, 15 duplication
rows, six verbosity rows, three complexity rows and three helper-clone rows.
Production growth handles explicit provider attribution, settlement and reporting.
The larger test clones share fixtures but assert distinct failure behavior; the
small clones construct tool requests or implement test traits. No suppression or
baseline change was made. The reviewer found no blocker and independently
reconciled the completed raw regression evidence before issuing QUALITY PASS.

## Independent reviews and self-audit

The final specification review is
`/home/shawn/demoncoder-check-tmp/native-stop-failure-spec-review.md`.
The quality review is
`/home/shawn/demoncoder-check-tmp/native-stop-failure-quality-review.md`.
Both passed for this prerequisite. The initial and request-builder specification
failures remain retained separately; neither was treated as approval.

The production rules self-audit found no unresolved revision for this prerequisite:

1. The native turn, provider, runner, owner and persistence paths were mapped.
2. Changes implement failed-turn observation and the concrete review findings.
3. Existing dispatch, owner fingerprints, runner and inspection helpers are reused.
4. Optional serialized fields preserve old records and unused matcher bytes.
5. Original errors survive; local request errors stay sanitized and unclassified.
6. Owner, workspace, allowance, role and request-boundary checks remain enforced.
7. Once reservations, uncertain effects and diagnostics remain durable without replay.
8. Deadlines, bounded patterns and shutdown cleanup are exercised.
9. The plan retains one active lifecycle item; no incomplete item was checked off.
10. Meaningful failing controls, five runners and the full regression suite ran.
11. Ignored tests, static failures and source/live evidence limits are explicit.
12. The implementation stays within the developer-selected complete commitment.
13. Independent reviews passed and the final checks required no further revisions.
14. The decisions and report name behavior, evidence and remaining work directly.

## Limits

This prerequisite covers returned native provider failures. An outer timeout that
drops the native future does not traverse this boundary. Session lifetime,
Interrupt, actual external source events, and public package matcher binding remain
work in the same commitment. Native host translations do not establish that an
external backend emitted a source event. Explicit configured model observers may
use their charged allocation; the no-retry requirement concerns task continuation.
