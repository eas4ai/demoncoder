# Managed Codex ordinary hook boundary

Status: the bounded source prerequisite passed specification and quality review.
Host ordinary integration and the complete commitment remain open.

This prerequisite follows the separate immutable ordinary-hook boundary and exact
acknowledgment decisions. It preserves the existing compaction patch as an earlier
stage and leaves normal upstream parser behavior unchanged outside private mode.
The private input retains actual source session/turn fields and carries a separate
transport delivery ID. Source hook-run identity and host lifecycle identity are
not manufactured from that transport ID.

The source worker demonstrated actual parser RED controls: existing Submit and
Stop parsers permit continuation after exit 1. Both tests failed without compile
errors in `managed-ordinary-red.log` under the scratch evidence root. The private
path now holds on failed or unacknowledged execution, as verified below.

The parent extended source preparation to apply and verify two fixed stages.
Eleven build tests pass: original-source preservation, reuse mutation refusal,
patch tampering, exact intermediate base and file preconditions, added files,
undeclared changes and external dependency changes. A controlled copy with only
the intermediate-tree guard removed failed the unchanged concealment test with
`RuntimeError not raised`; `managed-ordinary-build-mutation-1` retains that proof.
The original compaction patch and its provenance remain unchanged. The ordinary
patch and provenance bind the prepared source and qualified artifact. The build
entry point requires that provenance before compilation; unit tests use bounded
fixtures and actual replay checks verify the retained source.

SPEC review found that the existing source transport permits 1 MiB input but
only 64 KiB per captured output stream. The ordinary decoder now matches that
65,536-byte output limit. A decoder test for a violating 65,537-byte response
failed before the fix. Actual-command tests now accept exactly 65,536 bytes and
hold at 65,537 bytes. Host aggregate context can be
larger than that source transport limit, so subsequent host integration must
check serialized representability instead of truncating output or claiming full
package compatibility. Actual source failure, snapshot/isolation, compaction and
model-hook qualification passed; independent review results are recorded below.

The repaired source passes 191 hook tests. Strict Clippy still fails on four
existing `expect_used` diagnostics in unchanged `managed_compaction.rs`; it is
not a passing check. Patch SHA-256: `37605f53458e0d9c89e1d54fbeeac93b1d79e0b126be3bc0d9d832520b38ebf6`.
Binary SHA-256: `13a918d8881f6afe34956c9c42a09512f1e24bd5aa3089216ca31252b0c16cc1`.

The strict builder reproduced the same binary and passed all 191 hook tests.
A fresh two-patch application independently reproduced prepared tree
`93e336b8f510a18c24fb21fb27b7e1991233e1dd2f089089f5db3acc393a423c`.
Four actual model-hook isolation regression cases passed on this binary.

The first ordinary qualification run passed 25 startup cases and 38 runtime
cases, then failed its ambient positive control: only notification executed.
The fixture trusted the project but had not separately approved command hooks.
Both native and private comparison cases now supply the supported
`bypass_hook_trust` thread override for their harmless canaries. The corrected
native control executed all four expected tags. The complete new run passed
all 25 startup and 42 runtime cases;
the first failed run remains retained and is not an overall pass.

The snapshot case changes the declaration after process startup before opening
two threads. It proves startup retention, not refresh of an existing thread.
Registry unit tests separately exercise configuration refresh. Actual host
cancellation, durable ownership and recovery remain later integration work.

## Final development qualification

The final artifact is identified in `ordinary-build-receipt.json`; actual results
and their source-file digests are retained in `ordinary-qualification.json`.
These are development records, not Cairn evidence receipts.

- Builder: 11 tests passed, including exact intermediate-tree refusal controls.
- Source: 191 hook tests passed; fresh patch replay reproduced the exact tree.
- Ordinary backend: 25 startup and 42 runtime cases passed in
  `managed-ordinary-qualification-2`.
- Lifetime: three actual cases passed in `managed-ordinary-lifetime-2`.
  Submit and Stop interruption settled in about 0.11 seconds, with independent
  pidfd confirmation that each callback exited. The source and test owner
  survived; a late acknowledgment did not change the next turn.
- Refresh: after a first completed turn, the fixture invalidated the declaration
  and applied a non-default `hooks.state` edit with `reloadUserConfig:true`.
  It checked the applied status, read back the exact state, rejected both source
  reload-error warnings and completed the second turn on the same thread with
  both callbacks intact. This supplements the startup snapshot and registry
  refresh tests.
- Existing compaction: 17 startup and 36 actual source fault cases passed.
- Model-hook isolation: all four actual regression cases passed.
- Existing production compaction adapter: all 30 scenarios passed across three
  Rust tests in 356.30 seconds.
- Python Ruff, Black and Git whitespace checks passed.

Independent SPEC review verified final hashes and passed the 11 builder tests.
Removing the intermediate-tree guard in an isolated copy failed the unchanged
refusal test. Removing only the actual interrupt call in a separate lifetime
fixture failed its unchanged interruption assertion. The report is
`/home/shawn/demoncoder-check-tmp/managed-codex-ordinary-spec-review.md`.

The parent Ripwire report has quality exit 2 and test-gate exit 4. Findings include
new fixture branching, repeated explicit refusal cases, unittest methods reported
as dead code, and same-name attribution to the unchanged compaction qualifier.
Both focused edit checks returned zero. Per-row assessments are retained in
`managed-ordinary-parent-static-dispositions.md`; none is presented as a static
pass. Source Clippy's four existing compaction diagnostics remain unresolved in
this prerequisite and explicitly disclosed above.

Host ordinary admission, durable source ownership, cancellation/recovery through
the production adapter, model-hook environment isolation from the new ordinary
relay, complete package compatibility and the full commitment gates remain open.

## Independent quality review and self-audit

QUALITY passed with no blocking finding. The reviewer verified the complete
prepared tree, all ten changed source file hashes, both artifact identities,
the build receipt and all five retained qualification records. An actual forged
Stop acknowledgment held at one model request. Replacing that injected fault
with one valid acknowledged correction made the unchanged assertion fail with
`model requests 2 != 1`. The report and controls are retained in
`/home/shawn/demoncoder-check-tmp/managed-codex-ordinary-quality-review.md` and
`managed-ordinary-quality-controls-1`. Stale qualification wording found during
review was corrected before approval.

The final self-audit covered all production rules: this is a separate immutable
boundary using the existing bounded command runner, with exact provenance,
typed acknowledgment checks, cancellation cleanup and source-specific behavior.
No dependencies, credentials or unrelated policy were changed. Actual success,
failure, mutation, interruption, refresh and regression cases ran; independent
reviews attacked both the mechanism and its assertions. The plan retains one
item in progress, and no full-commitment item was marked complete for this
prerequisite. I am satisfied with the reviewed source boundary. Existing static
failures are disclosed, and production host integration and committed-tree Cairn
evidence remain required before the complete feature can be delivered.
