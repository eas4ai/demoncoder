# Prompt and agent runner prerequisite

Status: Implemented, verified and independently approved for the bounded
PreToolUse prerequisite. This does not complete the 41-requirement commitment.

## Scope and contract

The selected work integrates prompt and agent handlers with the existing
PreToolUse dispatcher. Prompt handlers have no tools. Agent inspection uses
captured evidence through host-owned read, list and search operations. The
owning allocation, selected model, immutable candidate and source-specific
verdict rules remain authoritative.

SUB-006 distinguishes native model calls from external backend invocations.
Each hook admission consumes its owning allowance under those controls.
Backend-internal request counts remain unknown where the protocol cannot
enforce them; an invocation limit is not an internal model-call cap. Reported
usage must not be charged again when presented as reviewer activity.

## Backend instruction isolation

Inspection found that an empty transport working directory alone does not
prevent Codex from reading global AGENTS.md. Its global instruction provider
is separate from the project-document byte limit. Relocating CODEX_HOME would
change keyring identity and could discard account restrictions. The recorded
[isolation decision](../decisions/isolate-model-hook-backend-instructions-without-relocating-subscription-credentials.md)
preserves the original authentication directory and adds a narrowly scoped
managed mode instead.

The rebuilt artifact retains the existing compaction protocol and exposes a
separate `demoncoder-model-hook-v1` capability. Its binary SHA-256 is
`80315a32acf1b625129a46b0bd75537cf76ef09701ae986154159076cc0b6aff`.
The build script verified the source tree, patch, external dependency records,
compiled artifact and both capabilities. Existing codex-hooks tests passed.
The new build receipt is retained under `backend-integrations/codex/`.

Executed controlled cases:

- Installed Claude 2.1.267: the ordinary setting-source control sent both
  synthetic global and workspace instruction canaries. Safe mode sent neither,
  while preserving synthetic subscription authentication and the explicit SDK
  inspection tool. Both cases completed two model requests and one inspection.
- Managed Codex: ordinary mode sent the global/base/developer canaries and ran
  the configured harmless notification command. Hook mode sent none and ran no
  notification. Both completed inspection and actual compaction through three
  local model requests, advertising only the snapshot read tool.
- Codex with a missing configured compaction-prompt file: the ordinary request
  was rejected before model delivery; hook mode completed without reading that
  unrelated file. The remote compaction request did not contain the custom
  prompt in either mode, so its absence alone was not counted as proof.

The repeatable cases are `tests/plugin_model_claude_isolation.py` and
`tests/plugin_model_codex_isolation.py`. Both passed; their formatting and Ruff
checks passed. The Codex qualification receipt is retained beside its build
receipt. These cases use actual backend executables with local model servers
and synthetic credentials. They do not establish live-provider evidence or a
complete installed management workflow.

Two probe expectations were corrected before recording passes: remote
compaction did not use the configured custom prompt, and an invalid configured
file rejected the protocol request without requiring the app-server to exit.
The replacement control checks the specific rejection and zero model requests.

## Remaining verification

The first rebuild passed all 17 startup, 36 backend compaction and 30
production-adapter cases. A final formatting-only correction passed targeted
rustfmt, fresh zero-fuzz patch application, exact-tree validation, all 179 hook
tests and a rebuild. The four isolation cases passed on this final artifact.
All 30 production-adapter cases, 17 startup cases and 36 manual/automatic
backend compaction cases passed on the final artifact. The retained build,
compaction and isolation receipts identify the same binary digest.

The affected regression suite passed and the final source manifest is recorded
below. Independent specification and quality reviews approved this prerequisite.

Other lifecycle events, remaining runner types, activation, public controls,
complete conformance and the commitment's live cases remain pending.

## Cross-filesystem staging regression

The broader command-runner suite reproduced a preexisting availability failure
when an ext4 snapshot was staged on tmpfs. Both objects had zero active statx
attributes, but their supported-feature masks differed (`0x303874` versus
`0x203070`). The same test passed with an ext4 temporary directory. That
environment change only diagnosed the defect; it did not resolve it.

The [recorded correction](../decisions/reproduce-active-snapshot-metadata-across-filesystem-types.md)
keeps source feature support in capture identity and freshness validation.
Materialization must reproduce active attributes, ownership, permissions, ACLs
and security attributes. It may differ in inactive optional filesystem support.
The corrected implementation passed the full 30-entry command suite with
default `/tmp` staging, 193 library tests, 12 snapshot tests and 15 workspace
tests. Two cleanup controls now violate active-attribute reproduction instead
of inactive feature support. Bypassing the final metadata check in an isolated
source copy made both controls fail; the shared candidate was not changed.

## Development diagnostics and retained checks

The first host compaction launch used an incorrect binary path and failed before
execution. The corrected command passed, and the final formatted artifact passed
the same 30 cases. A probe-supervision experiment was rejected by the model
fixture: the streaming supervisor intentionally kills its group at EOF, which
cannot satisfy the finite capability command's successful-exit contract. The
finite status path was restored without weakening its exit-status check. The
pinned capability branch returns before runtime/configuration initialization;
actual model sessions use the owned streaming supervisor.

Final artifact logs are `/tmp/plugin-model-codex-formatted-build.log`,
`/tmp/plugin-model-codex-final-fresh-prepare.log`,
`/tmp/plugin-model-codex-rustfmt-final.log`,
`/tmp/plugin-model-codex-final-isolation.log`,
`/tmp/plugin-model-codex-final-compaction.log` and
`/tmp/plugin-model-codex-final-installed.log`.
The metadata deletion probe is `/tmp/plugin-model-access-mutation.log`.

## Final development verification

The source manifest is `/tmp/plugin-model-rust-python-source.sha256` (27 files),
SHA-256 `65f2aef6bc6417f5a9ca4afd369a67f73c0c7aec2a07af603ecf4cfee82593b8`.
The implementer retained 43 log hashes in `/tmp/plugin-model-evidence.sha256`.
These identify development checks; Cairn evidence still requires a committed tree.

- Model runners: 16 passed, including 20 nested external transport controls.
- Shared regressions: 193 library tests, 30 command-runner entries, 12 snapshot
  tests, 28 language-service tests and 15 workspace tests passed.
- Additional regression batch: 186 passed across 23 targets, eight ignored.
- Production terminal: output limits (three tests), VERIFY-002 cancellation and
  retained-result cases, and VERIFY-005 cumulative allowances passed.
- Installed Claude: all 18 manual/automatic compaction controls passed against
  the current host adapter. This is additional to the two isolation controls.
- Formatting, Clippy with all targets/features and warnings denied, Python
  formatting/lint/syntax and whitespace checks passed.

Ripwire remains nonzero: quality-delta exits 2 with 143 rows and 50 gating
findings; test-gate exits 4 with 34 target obligations and 365 impacted symbols
without mapped tests. `/tmp/plugin-model-static-dispositions.txt` retains each
quality row and its assessment. The real production increases include five
branches and 34 lines in Codex connection setup, and 14 lines in policy setup.
Other entries include fixture duplication, short-horizon churn and ambiguous
same-name attribution. None is reported as a static pass. The independent
quality review assessed and accepted these costs for the bounded prerequisite.
The committed-tree live Oracle check remains part of the full commitment gate;
these controlled checks do not replace it.

## Independent specification review

The reviewer found no concrete deviation within the bounded PreToolUse scope.
All 27 source hashes matched before and after review. The reviewer independently
ran 16 model tests, six metadata tests, 30 command tests, 12 snapshot tests and
four actual managed Codex isolation controls. The complete contract trace and
limits are retained in `/tmp/plugin-model-spec-review.md`. No source edits or
Cairn checks were made during that review.

## Independent quality review and self-audit

The quality reviewer found no Critical, Important or Minor defect. Twenty new
native verdict/inspection controls passed across OpenAI and Anthropic. They
covered duplicate/trailing JSON, invalid verdict types, invented authority,
traversal, NUL and extra inspection arguments, and attempted approval after a
failed inspection. Violations made no mutation and sent only one model request.

An actual Codex control deliberately removed managed isolation in a temporary
harness. Its canary/notification assertions failed. Restoring the original mode
passed. Six metadata tests passed, and all 27 source hashes plus the managed
binary hash remained unchanged. `/tmp/plugin-model-quality-review.md` records
the independent source review, static findings assessment, commands and limits.

The final self-audit checked the production standard against this bounded change.
The implementation uses existing authority, accounting and durable receipts;
input/output, inspection, time and cleanup have enforced bounds. Success and
relevant violations were executed, metadata failure controls were demonstrated,
and both independent reviews approved the same source. I am satisfied with this
prerequisite under the standard and know of no unresolved defect in its reviewed
scope. Nonzero static diagnostics and unexecuted live/full-commitment work remain
explicit above; none is counted as completed delivery.
