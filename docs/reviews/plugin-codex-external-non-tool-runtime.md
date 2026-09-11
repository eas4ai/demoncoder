# Codex external Submit and Stop integration

Status: implementation and required development regressions passed; final review reconciliation follows.

This prerequisite connects the qualified private Codex ordinary boundary to the
existing typed lifecycle, runner and durable owner ledger. It follows
`authorize-external-non-tool-callbacks-through-their-actual-lifecycle-owner` and
`retain-the-actual-codex-command-request-alongside-source-hook-identity`.
The original source boundary passed specification and quality review, then its
review was reopened when host testing exposed preview truncation. Renewed source
qualification and host review are required for the repaired candidate.

## Observed missing behavior

The initial actual production test ran the new pinned Codex binary with the
unchanged host fixture. It made one model request with no peer errors, then failed
because the host recorded zero lifecycle callbacks instead of two. The assertion
failed at `tests/plugin_external_non_tool.rs:304`, without a compile or launch
failure. Evidence: `/home/shawn/demoncoder-check-tmp/managed-ordinary-host-red-1`.

## Identity and continuation contract

Keep the adapter's actual numeric turn/start request, the source's native hook
run ID, the private delivery UUID and the ordered host occurrence distinct.
The new optional command request metadata must preserve old receipt decoding
without granting missing authority. Source callbacks must match their active
hook notification, source session/turn and adapter-owned command. Only the
private transport envelope is removed before closed upstream schema validation.

A post-tool correction may admit its own Submit callback before the exact
turn/start reply arrives. It must retain the reserved invocation and original
correction frame. After acknowledgment, the retained request, thread and turn
must match exactly. Stop correction remains subject to the original allowance,
deadline and correction limit. Required failures and responses exceeding the
source's 64 KiB transport limit must hold; context must not be truncated.

## Verification plan and limits

Required actual cases include allow, repeated turns, Submit denial, Stop
correction and exhausted correction budget, cancellation/shutdown, malformed
results, timeout, source death and mixed post-tool correction. Rust tests must
also attack stale/forged identities, replay, uncertain recovery, response bounds
and model-hook environment isolation. Existing Claude and compaction behavior
must remain covered. Independent specification and quality reviews follow the
completed implementation and actual cases.

The shared Python peer now pins the newly qualified Codex artifact and can emit
a real dynamic write call and capture the direct app-server PID. Its source
ordinary and lifetime checks passed again: 25 startup, 42 runtime and three
lifetime cases. Exact helper copies are retained beside those runs. Subsequent
host-only context cases add full 60 KiB input preservation and 70 KiB overflow
checks; they do not alter the source peer behavior used by those captured runs.
All evidence here is controlled transport until the full commitment's live
checks run. Public activation, all event/package components and Cairn gates
remain outside this prerequisite and still required by the selected commitment.

## Initial integration probe

The first compiled integration probe held before model work but failed to deliver
any runner callback. The diagnostic trace showed a valid private hook start,
followed seven milliseconds later by source status `failed`: the required relay
command exited unsuccessfully. The host reported a completion mismatch. The
source made zero model requests and the peer reported no error. This is a failed
probe, not a passing denial case. Diagnosis found the client socket pathname
exceeded the Unix address limit; the server already used a short descriptor
alias. The recorded repair pins the parent directory and exact socket inode
before connecting through a client-owned descriptor alias, preserving peer and
token authentication. Long-path and symlink controls are required.
Evidence: `managed-ordinary-host-probe-1-pass` and
`managed-ordinary-host-probe-1-trace` under the scratch evidence root.

The repaired socket transport passed actual normal delivery, Submit denial and
Stop correction, then mixed post-tool correction and Submit cancellation.
The full-context probe exposed the source preview issue described in the source
review; its assertion is retained. Context overflow cases now distinguish a
60 KiB quote-heavy payload exceeding the JSON transport limit after settlement
from a 70 KiB raw contribution exceeding the lifecycle bound before settlement.
Both must retain Pending delivery and reject replay, with their actual settlement
states preserved. The empty terminal-response probe failed at the old host guard
with only Submit delivered; its required null Stop observation is being added.

The private source always registers both events. Partial user registrations must
therefore retain an empty authenticated source observation for the absent event,
without inventing a handler or changing native unregistered events. A separate
recorded decision requires owner/continuation validation before any allow reply.
Actual Stop-only and Submit-only post-correction cases are being added.

## Current repaired candidate checks

All six focused actual-backend cases pass with source binary `c4d77a24`: empty
Stop, Stop-only and Submit-only post-correction plans, full admitted context,
serialized response overflow and lifecycle contribution overflow. Evidence is
`managed-ordinary-host-probe-4-<case>` under the scratch evidence root. The
empty-owner tests first failed for the intended missing behavior; the updated
317-test library suite and all-target Clippy pass. Full Rust and final actual
backend regressions are still running or pending; these are development checks.

The repaired source passes three lifetime cases, 17 compaction startup and 36
compaction runtime cases, and four model-isolation cases. Its first complete
ordinary run failed on a fixture TLS EOF during the unknown-field case. The
source held the malformed acknowledgment with zero model requests. An unchanged
Submit/Stop retry passes; the failed run remains retained and the full unchanged
run is being repeated. No TLS error assertion was suppressed.

Fresh host specification review passed with no concrete blocker. It independently
audited the six actual probe captures and full context string, and exercised the
Linux pinned-socket replacement premise outside the candidate. The retained
report is `managed-codex-host-spec-review.md`. Its approval explicitly awaits the
full regression results and separate source review before delivery. Final quality
review remains pending. Parent Black, Ruff and whitespace checks passed.

## Final matrix and controlled-peer finding

The full Rust suite passes 815 tests, with 17 intentionally ignored cases across
44 suites. Final matrix 1 passes all 22 Claude cases and 18 of 21 Codex cases.
Three Codex cases fail the peer check despite passing Rust owner assertions and
zero model requests. Independent controls expose an unused-connection EOF
misclassified as oversized and a truncated CONNECT header accepted as complete.
Their strict repair passes unit and specification review. Nine actual phase
probes then pass eight cases and isolate one failure to TLS-handshake EOF before
HTTP begins. The additional recorded transport decision preserves this narrow
abandoned-connection fact separately, while retaining malformed, other TLS and
application errors as failures. Final quality approval remains held until the
repair, failure controls and affected regressions complete.

The corrected final matrix passes all 43 actual-backend/local-peer cases: 21
Codex and 22 Claude. `managed-ordinary-final-host-matrix-2/results.json` binds
the unchanged host binaries and corrected helper. The fixture’s 16 focused
tests and renewed specification review pass; independent quality controls also
verify concurrent journal ordering and strict persistence failure. Final source
and host compaction regression reconciliation remains pending.

## Static diagnostics

Final Black, Ruff and whitespace checks pass. Ripwire quality delta exits 2
with 103 rows and 35 gating findings; its test gate exits 4. These are retained
diagnostics, not passing checks. The existing Codex adapter grows to supervise
ordinary callbacks alongside tool and compaction state. Small backend-specific
provenance and prepare/sent functions remain explicit while sharing the runtime
validator. The private peer adds ordered, bounded connection cleanup and journal
handling. Review found no concrete factoring defect requiring a new abstraction.
Test setup and server lifecycle overrides are reported as dead or duplicated
by static name matching despite their executed controls. No findings were
suppressed and no baseline was changed. The qualified lifetime edit check passes;
an earlier unqualified `session` invocation was ambiguous and was corrected.
Exact outputs are `managed-ordinary-peer-final-{quality-delta,test-gate}.log` and
`managed-ordinary-peer-final-edit-check-lifetime.log` in the scratch evidence root.

## Final verification and limits

All required checks for this prerequisite pass: 815 Rust tests (17 ignored),
all-target host Clippy and formatting, 16 peer tests, Black/Ruff, 43 actual
Submit/Stop cases and 30 actual host compaction cases. Source qualification
also passes 25 startup and 43 ordinary cases, three lifetime cases, 53 source
compaction cases and four model-isolation cases. Reviews retain the controls,
static diagnostics and exact evidence rather than treating source callback
success as host delivery. The helper repair changes no production hook contract.

This completes the external Submit/Stop prerequisite, not the selected
41-requirement commitment. Remaining lifecycle families, package activation,
public controls, full compatibility, installed/live workflows and Cairn receipts
remain required. No live-provider or whole-product completion is claimed.

## Final review and production self-audit

Fresh host, source and fixture specification reviews and the combined quality
review pass with no unresolved finding in this prerequisite. Rules 1–4: the
change stays within the recorded lifecycle decisions, uses existing ownership
and preserves older receipt decoding. Rules 5–8: authentication, exact identity,
frame bounds, cancellation, replay holds and bounded cleanup have failing and
corrected controls. Rule 9: the plan retains exactly one in-progress lifecycle
item; only the verified Submit/Stop items are completed. Rules 10–11: executed
checks and hashes are recorded, controlled/live evidence is distinguished, and
nonpassing static/source-Clippy diagnostics remain disclosed. Rules 12–14:
independent review resolved the observed gaps; documentation names remaining
work and does not claim the full product is complete. No further revision is
needed for this bounded implementation. Cairn evidence and the rest of the
selected commitment remain separate required work.
