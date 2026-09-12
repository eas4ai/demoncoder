# Exact operation budget and usage attribution

Status: implemented and verified as a bounded prerequisite. Fresh specification
and quality reviews pass. Lifecycle dispatch and the full commitment remain open.
Session-funded model, HTTP and MCP runner execution is not enabled here.

## Reviewed behavior

New causal owners and model/backend/tool operations retain their original task
allocation, explicit session grant or unfunded state. Checked allocation epochs
distinguish taskless equal-limit replacements. EventSink forwards the exact
invocation and phase for usage. Known and missing usage settles into the original
active or retired grant after completion, cancellation, reconciliation or expiry.
Retired accounting cannot authorize new execution.

The review examined allocation/delegation transitions, model/backend admission,
hook/service inheritance, recovery through actual storage reopening, child
cancellation, tool effects and replay, arithmetic failures and inspection output.
Up to 32 referenced retired allocations retain canonical counters without silent
eviction. Archives remain frozen historical snapshots. Ambiguous legacy or invalid
recipients stay visibly unresolved without choosing a newer task or session grant.
Fallible accounting and allocation transitions preserve prior state. Exact tool
replay returns its original result/reference without another debit. Snapshot tools
continue through the existing durable tool ledger.

The ToolStarted fallback test covers compatibility notices. The actual production
ToolExecutor emits that notice after durable admission and before the effect;
scoped tool sinks skip duplicate observation. This is not qualification of an
actual backend notice producer or a new post-effect accounting mechanism.

## Closed Oracle findings

Ordinary Oracle review could wait for actual ToolReview delivery, then lose the
original tool identity in a generic child sink and select a replacement allocation.
VERIFY-005 includes Oracle calls. The correction captures the pending admitted
tool before adapter opening and rechecks its source and original budget under
model/backend admission locks. General child creation clears Oracle-only authority.

The first correction wrongly required an incomplete source host. Native model
responses finish before their returned tools execute; command hosts also start
complete. The pending admitted tool is the live owner. The corrected guard permits
both normal sources while preserving exact identity and recovery checks and
rejecting completed/reconciled tools. Both findings are closed on the reviewed
source. Review-only access, configured connection, timeout, drain, close and
original errors remain intact.

Compiled controls first demonstrated an actual replacement-funded Oracle request
and then the valid completed-host rejection. Corrected tests delay real ToolReview
delivery and check request files, target-file effects, original budgets and exact
12-input/8-output usage. Replacement sends no Oracle request and incurs no new
debit. The focused Oracle filter passed five tests: three relevant runtime tests
and two existing UI/settings matches.

## Executed verification

| Check | Result |
| --- | --- |
| Corrected `cargo test --all-targets` | Exit 0; 45 suites, 919 passed, zero failed, 17 ignored; 615.24 summed suite seconds. |
| Corrected `cargo clippy --all-targets -- -D warnings` | Exit 0. |
| Corrected `cargo fmt -- --check` | Exit 0. |
| Separate final-binary `tests/output_limits.py` | Exit 0; three PTY cases passed in 4.893 seconds. |
| Fresh specification and subsequent fresh quality reviews | PASS; all 22 changed source/test files reviewed. |
| Ten final repetitions of the prior crash test | Ten passed with the effective Cargo environment. |
| Ripwire quality delta | Exit 2; 138 rows, 45 gating rows. Not a pass. |
| Ripwire test gate | Exit 4; 42 named tests, 367 symbols unmatched by its test-name heuristic. Not a pass. |

The parent independently parsed the final full log and checked source, main-binary
and Cargo configuration stability. Quality review independently checked raw logs,
both binaries and all 22 source hashes. Effective Cargo test threads were 12,
without a serial command override.

Compiled failing controls also retain four initial attribution/arithmetic failures,
two native-owner inheritance failures and the compatibility-notice failure. The
initial correction passed all four original cases. Fixture repairs moved funding
before causal owner creation while retaining gate-count, no-effect, file-content
and debit assertions. Compiler errors, intermediate fixture failures, earlier
candidate passes and the deliberately interrupted defective Oracle run remain
labeled development evidence, not final-candidate results.

Static review found no concrete defect requiring a source change. Dispositions
cover moved dispatch complexity, required source/session parameters, distinct
forwarding guards, serde/test reachability, churn and two filename collisions.
Raw diagnostics remain retained. No suppression, acknowledgement file, test
exclusion or metric-only rewrite was introduced.

## Validation incident and limits

One intermediate library run aborted with SIGILL. Retained core diagnostics show
a Rust BTreeMap navigation panic during session serialization and abort during
cleanup. The cause remains unconfirmed. The original binary was rebuilt before
its digest was retained; no later digest identifies that crashed candidate.
No speculative source or toolchain fix was made.

The exact test,
`native::non_tool_tests::native_turn_text_resets_and_excludes_plugin_output`,
subsequently passed 20 direct repetitions, ten effective-Cargo-environment
repetitions on the earlier rebuilt candidate, and ten on the corrected final
candidate. Both later full suites passed. This records non-recurrence, not proof
of a fix. The developer restored Cargo Wizard's temporary nightly setting; that
restoration does not establish the crash's cause either.

Provider usage aggregation remains unchanged. No duplicate-safe backend totals or
knowledge of backend-internal model calls is established. The 17 ignored tests
remain ignored. Earlier installed/live-source evidence retains its original
candidate. These working-tree checks are not Cairn receipts or completion of the
full skills/plugins/hooks commitment.

## Evidence identity

Artifacts are retained under
`/home/shawn/demoncoder-check-tmp/operation-budget-attribution-`.

| Artifact | SHA-256 |
| --- | --- |
| `all-targets-oracle-final.log` (95,998 bytes) | `60d122efa56c7aec9faab1a59a98c378ef8e0ffabab278db44fd1d5666de429a` |
| `source-oracle-corrected-stable-sha256.json` (22 files) | `63d381e6a282814829024a88b846c4b2dc0b0d97da78e73c4904ab5d5c36a022` |
| Main binary | `fb4b8ccba7d7056391eb6eec00177d4b39a997f8671070be869f086429869a64` |
| Library test binary | `fe32ee85e5c16c4b95417c0175b0be51628446ae5ffd499621c34d0d126dfe73` |
| Restored Cargo config | `03861e19e619274355ae786816cd1a4a1d27ccecd67b38533e04f9c2602f2f0b` |
| `evidence-index.json` (145 artifacts) | `4229ce808099b8d8c83b4e166028005f3d157d6c39f84981f69b44d7315763b1` |
| `quality-review.md` | `d8b2f32d2254b7899847520ccb6ef4c5a9e9e21a1a068b80ff0079829eb1a8b2` |

## Production-rule self-audit

| Rule | Assessment |
| --- | --- |
| 1. Understand before editing | Mapped causal owners, grant transitions, usage, recovery and actual Oracle delivery order. |
| 2. Smallest coherent change | Reused the ledger and session identity with focused accounting and Oracle validation modules. |
| 3. Maintainability | Named types separate grant identity, admission and settlement; no second accounting service. |
| 4. Boundary contracts | Defaults read old records without inventing ownership; constructors and callers changed together. |
| 5. Errors and secrets | Checked failures preserve state; bounded structured diagnostics expose no credentials or raw payloads. |
| 6. Security | Attribution grants no authority. Live owner, gate, key, confinement and recovery checks remain. |
| 7. Survivable state | Exact late settlement, frozen archives, bounded retirement and original-result replay preserve recovery. |
| 8. Reliability | Bounded ledgers and fixed-size usage rollups; no new unbounded task, queue or retry. |
| 9. Track work | This prerequisite is verified; lifecycle dispatch remains the sole active plan item. |
| 10. Verification | Compiled controls, boundary regressions, full suite, Clippy, formatting, PTY and independent reviews ran. |
| 11. Honest reporting | Static non-passes, ignored tests, earlier candidates and the unexplained crash remain explicit. |
| 12. Partnership | Followed recorded scope and preserved developer configuration restoration and original evidence. |
| 13. Final audit | All 14 rules reviewed; no further source revision identified in this bounded prerequisite. Broader work remains open. |
| 14. Plain writing | Records name original operations, allowances and observable effects, separating evidence from inference. |
