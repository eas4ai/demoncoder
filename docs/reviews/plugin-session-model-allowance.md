# Native session model allowance integration

Status: implementation, corrected regression checks and fresh specification and
quality reviews pass on source manifest2. This is a bounded lifecycle prerequisite;
the full skills/plugins/hooks commitment remains open.

## Behavior

Actual native SessionStart and SessionEnd can execute synchronous Prompt and Agent
hooks from the session's original explicit grant. A connection or imported model
tag cannot supply funding. Preparation, model/backend admission, snapshot tools
and delivery use the same original reference and live occurrence. Task expiration,
stop, acceptance, replacement and archive do not transfer or replenish that grant.
Ordinary Task/Unallocated owners never fall back to session funding.

Native calls and host-admitted external backend invocations consume model slots.
Session backend invocations have a separate durable counter and inspection label;
unrelated delegation exhaustion or counter overflow neither blocks nor funds them.
Counter arithmetic is checked before debit. Legacy session records default the
new counter to zero without restoring a live lifetime. Existing adapter aggregation
and unknown backend-internal usage remain unchanged.

Prompt remains tool-free. Agent uses retained read-only snapshot tools through
existing durable tool admission, charging one session tool slot per fresh request.
The original cumulative grant, actual occurrence and configured handler timeout
bound execution and cleanup. Startup retains its 30-second boundary, end retains
five seconds, and the application retains its eight-second native shutdown reserve.
End and resume grant no fresh counters or time. Recovery holds, cancellation,
stale keys, expired occurrences and deserialized references cannot authorize work.

Session model results remain attributed observations. A false or invalid verdict
cannot start another task turn, accept work or veto shutdown. HTTP/MCP and
asynchronous session commands remain unavailable pending their lifetime integration.
Native host execution does not qualify an external backend's lifecycle source;
source applicability and explicit native conversion requirements remain unchanged.
Public activation and complete package workflows remain separate pending work.

## Findings and corrections

The initial compiled control observed zero model requests where a funded no-prompt
session required startup and end requests. Dispatcher, durable reservation and
model preparation/admission all retained command-only or task-only restrictions.
The corrected complete runner target exercises the real native lifetime and
controlled requests through both native protocols and configured external hooks.

The next compiled control showed legitimate session work blocked by task expiry
and session backend calls charged to the global task/delegation count. Exact grant
post-checks and a distinct session counter correct those paths. The first broad
run then caught two existing safeguards: 378 library tests passed and two failed.
Unallocated operations had lost their previous conservative task-clock guard.
That guard is restored without assigning or debiting task funding; all 25 original
tool-operation tests pass with unchanged assertions.

A separate compiled control showed a held model owner could deliver a verdict
between periodic owner checks. The runner now checks live ownership before final
verdict parsing and after adapter close, while retaining usage settlement.

Fresh specification review found two further issues and verified their corrections:

- **S1, post-persistence tool authority:** EventSink checked the owner before
  durable transitions, but their post-check covered only the cumulative grant.
  RED6 directly observed a before hook after checkpoint revocation. RED7 directly
  observed persisted successful snapshot bytes after the effect checkpoint expired
  the occurrence while its broader grant remained live. Fresh begin, admission,
  effect and observer startup now recheck exact live hook delivery after persistence.
  Replay and result/usage/observer-outcome settlement remain available after expiry.
  The complete test covers four boundaries with both deadline and cancellation,
  valid controls, retained debit/effect markers, no revoked inspection/presentation,
  preserved original results and late exact usage settlement. Its checkpoint probe
  is test-only, thread-local, workspace-scoped and one-shot.
- **S2, additive serialized shape:** The old complete-object equality assertion
  expected legacy `{allocation}` to reserialize unchanged. The updated assertion
  includes `backend_invocations: 0` and still proves legacy missing-field reading
  and the complete object shape. It was not reduced to an allocation-only check.

Both specification findings are closed on manifest2. Initial fixture compiler
errors and a rejected native concurrent-group fixture remain separate development
evidence. Sequential sharing is tested through actual native execution; a runtime
race independently proves atomic admission of the last slot. The failed first
broad run is retained and never counted as a pass.

## Executed verification

Raw evidence is retained under
`/home/shawn/demoncoder-check-tmp/session-model-allowance-`.

| Check | Result |
| --- | --- |
| Corrected `cargo test --all-targets` (`full2`) | Exit 0; 45 suites, 938 passed, zero failed, 17 ignored; 640.74 summed suite seconds. |
| `cargo clippy --all-targets -- -D warnings` (`clippy2`) | Exit 0. |
| `cargo fmt -- --check` (`fmt-check2`) | Exit 0. |
| Focused runtime controls (`unit3`) | Seven passed. |
| Original tool-operation target (`canaries2`) | 25 passed; original assertions unchanged. |
| Complete runner and allowance targets (`focused4`) | 31 model-runner and 10 session-allowance tests passed. |
| Parent's separate final-binary output-limit PTY suite | Three passed in 4.076 seconds; exit 0; binary unchanged before/after. |
| Fresh specification review | PASS on all 15 manifest2 source/test files; S1 and S2 resolved. |
| Fresh quality review | PASS on manifest2 and final evidence; no unresolved source finding. |
| Qualified Ripwire edit check | Exit 0; the earlier ambiguous unqualified query exited 1 and is retained. |
| Ripwire quality delta | Exit 2; 77 rows, including 33 gating rows. Not a pass. |
| Ripwire test gate | Exit 4; 42 suggested paths and 499 unmatched symbol entries. Not a pass. |

The parent independently parsed the final full log, matched all 15 source hashes
and checked final main-binary and Cargo configuration stability. The restored
Cargo configuration retains 12 test threads without a serial command override.
The separate PTY run uses that exact compiled binary directly. Full2 showed no
recurrence of the previous prerequisite's SIGILL; its cause remains unconfirmed.
No speculative crash fix or managed-backend cache rebuild was made here.

Static dispositions cover required dispatcher branches, separate backend counter
validation, test matrices, dynamically called tests/traits, short fixture matches,
churn and a misplaced `overview` location caused by a name collision. The raw
findings remain retained for review. No suppression, acknowledgement file, test
exclusion or metric-only rewrite was introduced. These mapping/quality diagnostics
are not complete coverage or clean static gates.

## Evidence capture correction and limits

The initial evidence wrapper captured a registry credential through a broad
Cargo environment filter. The parent printed that metadata before noticing the
credential. Seven local metadata files were sanitized, first by redacting values
and then by removing non-allowlisted keys. Before, intermediate and final hashes
are recorded in `metadata-redaction-audit.json`. Raw test logs and exit statuses
were unchanged. The earlier conversation output cannot be erased by local edits.

Capture now uses an explicit non-secret allowlist. Parent and worker scans found
no current credential-named environment values in the checked task artifacts;
this is bounded verification, not proof about every possible secret or downstream
retention of the earlier tool output. This process error remains documented.

The 17 ignored tests remain ignored. Controlled backend requests are controlled
transport evidence, and trusted storage seeding proves persistence rather than
public activation. Earlier installed/live-source qualifications retain their
original candidates. These working-tree tests are not Cairn evidence receipts or
acceptance of the complete commitment. Synchronous filesystem operations retain
the existing cooperative timing limits; an outer timeout cannot preempt arbitrary
blocking kernel I/O.

## Candidate and evidence identity

| Artifact | SHA-256 |
| --- | --- |
| `source-manifest2.json` (15 files) | `1a079db62ba2ba467ad634022ac76a223fbdf4bca8ec9f365084dabfcad9df14` |
| `full2.log` (98,265 bytes) | `12d2a6225d738da6c65d68826f835ee165f8ee90dcb7b35cda881315b145895f` |
| Main binary | `dd95d0a514e1f25180b8242dd654d79c810602e2d942e4905e6e1031e7e816e9` |
| Restored Cargo configuration | `03861e19e619274355ae786816cd1a4a1d27ccecd67b38533e04f9c2602f2f0b` |
| Fresh specification report | `36c27e3a68309632e8df036b972e173c1c6d0070725c48a1047367ee0e0b5d1d` |
| Fresh quality report | `fcd0843efa08e700fe5e3b466d01dc7aef067c109dedbbb4386c5ea14027725b` |
| `evidence-manifest2.json` (15 sources, 146 artifacts) | `0df0b9d1971b768d8a6cd7208de26664173e89fb39dbc954370d4214e6fcf807` |

## Production-rule self-audit

| Rule | Assessment |
| --- | --- |
| 1. Understand before editing | Mapped actual lifetime, preparation, admission, persistence, effect, delivery and settlement paths. |
| 2. Smallest coherent change | Reused original grant references, operation ledger, runner leases and snapshot tools. No new dependency or transport enablement. |
| 3. Maintainability | Funding and live authority remain explicit. Focused test modules hold the matrices; checkpoint injection is absent from production. |
| 4. Boundary contracts | Validated original owners and defaulted additive counter; retained complete legacy shape and resume tests. |
| 5. Errors and secrets | Product errors remain bounded; the evidence-capture mistake was disclosed, sanitized and corrected with an allowlist. |
| 6. Security | Prompt has no tools; Agent remains snapshot-only. Final owner checks follow persistence and precede effects. |
| 7. Survivable state | Atomic counters, preserved debits, original-result replay and late exact settlement survive cancellation/recovery. |
| 8. Reliability | Existing occurrence limits and owned cleanup remain; no new unbounded task or retry. Blocking-I/O timing limits stay explicit. |
| 9. Track work | Lifecycle dispatch remains the sole active plan item; this prerequisite has completed implementation and verification. |
| 10. Verification | Compiled failures demonstrate actual requests, effects and delivery gaps; corrected focused/broad checks and PTY ran. |
| 11. Honest reporting | Earlier failures, ignored tests, nonzero static exits, capture error and source-qualification limits remain visible. |
| 12. Partnership | Followed recorded funding policy, preserved restored Cargo settings and kept broader commitment work open. |
| 13. Final audit | All 14 rules assessed; both independent reviews pass with no unresolved source finding. The historical capture error and remaining commitment work stay explicit. |
| 14. Plain writing | Records identify the original grant, live owner, observable effect and limits of each check. |
