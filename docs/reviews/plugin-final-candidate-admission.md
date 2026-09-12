# Final-candidate tool admission

Status: Approved by independent specification and quality reviews after both corrections.
Base: `88e3e4314e675e2ea243bcac162dafaee2ca2c23`.

This review covers the pre-tool admission prerequisite within the complete
skills-plugins-hooks commitment. It does not establish installed package
execution, the five production runners, public activation or developer-answer
controls, remaining lifecycle events, or full commitment completion.

## Scope examined

The implementation attaches an immutable host-selected plan to ToolExecutor.
It retains hook invocations in the existing tool receipt, separates original
requests from inspected candidates, and checks the frozen candidate before the
actual effect. SharedRuntime owns a workspace-specific mutation boundary shared
with executors that have no plugin plan. Final snapshot inspection runs on a
bounded cancellable worker while retaining that boundary.

Imported source handlers remain combined handlers. Refreshing their stale pass
requires an explicitly declared read-only endpoint. A different final aggregate
read set also requires revalidation, even if arguments did not change. This
conservative case has a regression test and a retained needs-revalidation hold.
Actual mutating runners must join the same host boundary when integrated.
An application lock does not provide atomicity against external processes.

## Executed development checks

The initial candidate passed these development checks before independent review:

- `rtk cargo test --lib --test plugin_admission --test plugin_tool_receipts
  --test tools --test tool_extensions`: 206 passed across five suites.
- `rtk cargo clippy --all-targets -- -D warnings`: passed.
- `rtk cargo fmt --all -- --check`: passed after restoring the deletion probe.
- `git diff --check`: passed.

Two earlier broader runs each passed 388 tests, with four ignored, across 17
suites. They preceded the final candidate-display correction. The final focused
run above covers that correction. These are controlled development checks, not
live-provider evidence or Cairn receipts.

After correcting S1 and S2, the implementer ran the five suites above plus
`host_guard`, `developer_access` and `worktree_access`: 250 passed and one
external-assessment case remained explicitly ignored, across eight suites.
The command exited zero. The parent examined the retained complete test log and
its per-suite results. All-target Clippy, formatting and whitespace checks also
passed on this corrected candidate. These results supersede the earlier runs for
the changed path resolver and result settlement.

The deterministic final-rescan regression pauses the last scan, replaces the
inspected target or parent, then checks actual write/create admission in both
access modes. Removing only the post-await target-anchor check made this test
fail with exit 101 in the confined new-file case. Exact source restoration made
it pass, including in the final suite. The parent examined the failure capture.

The deletion probe removed only the final
`admission.validate(self.guard.clone()).await?` call from ToolEffect. The unchanged
`shared_host_boundary_serializes_separate_executors_but_not_other_workspaces`
test failed: the stale candidate wrote seven bytes to `guarded-output`. Exact
source restoration made the same test pass. The parent examined the captured
failure output; independent review remains responsible for assessing the test.

## Static findings

Ripwire's final effect edit check reports an unchanged callable contract and no
incompatible calls among five discovered callers. This does not prove behavioral
compatibility.

The corrected candidate's quality report exits 2 and retains 131 rows, including
28 gating rows. The parent examined its complete finding list. Token-normalized duplicate groups
include unrelated fixture cleanup/accessor methods, small EventSink/runtime
wrappers, test call constructors and SHA digest helpers. Sharing those across
unrelated domains solely to remove a finding would add coupling. The nested
canonicalization function also appears as a clone of its enclosing function.
Churn counts describe commit history, not repeated failed implementation attempts.

The substantive complexity findings remain for quality review:
ToolExecutor.execute grows from 186 to 200 lines and complexity 18 to 21;
Admission.run is 114 lines with complexity 27. Outcome interpretation is in its
own module. The reviewer must assess whether the remaining phase orchestration
is understandable and adequately exercised. No static quality pass is claimed.

S1 adds a 65-line candidate resolver and increases file opening from 51 to 65
lines to create through the pinned parent. Quality review must examine this
boundary. New clone rows also compare an unrelated manager destructor with
target binding, and two small rewrite fixtures; these are not reasons to share
logic across unrelated domains. The alias and race test matrices carry increased
nesting and complexity because they cover both access modes and target states.

The test gate exits 4. Its complete report contains 28 test files and 188 symbols
without a discovered test path, with no capped rows. Name-based reachability
includes broad adapter, UI and subagent paths. It also lists the new admission
unit tests themselves and admission methods exercised by integration tests.
It supplies obligations for review rather than a passing result. Full installed
and live acceptance remains part of the parent commitment.

## Independent reviews

The independent specification review did not approve the candidate. It ran the
206-test focused suite successfully, then reproduced both findings below with
two additional tests against the actual library. Both probes failed with exit
101. No shared implementation files changed during review.

Re-review approved the corrected prerequisite and closed S1 and S2. The reviewer
independently reran the original probes and an additional read/edit alias matrix:
three scratch tests passed. All seven formerly bypassing aliases now reach the
canonical denial and leave original bytes unchanged. The matrix also covers
permitted operations, replay and original-request retention. The concurrent
failure probe retains the raw denial and typed decision while recovery stays held.
The reviewer also ran 28 plugin integration tests, the final-rescan regression,
and five runtime admission tests; all passed. No adjacent confirmed findings
remained. A fresh code-quality review followed this specification approval.

The independent quality reviewer approved the bounded prerequisite without
remaining findings. It examined target resolution, descriptor binding, final
publication, cancellation ownership, receipt validation and prospective bounds,
and protocol cohesion. It independently ran 28 plugin integration tests, five
runtime admission tests and two rescan tests; all passed. Whitespace checks
passed and all 15 source hashes matched the reviewed candidate manifest.

The reviewer assessed the remaining protocol complexity as cohesive orchestration
and found no useful simplification merely to lower the static counters. This is
a reasoned disposition of the nonzero reports, not a static-gate pass. Production
runners, activation, extension/LSP integration and the remaining lifecycle work
still require their own implementation and evidence.

### S1 — P1: Path aliases bypass a required matcher

Closed by independent re-review. The independent specification reviewer originally reproduced a real write bypass at
`src/plugins/dispatch.rs:194`: the matcher compares raw path spelling, while file
admission accepts normalized components. A gate matching `generated/file` denies
that spelling, but `generated//file` skips the gate and creates the same file.
The bypass called the gate zero times and wrote `written` to `generated/file`.

The reviewer ran a scratch crate against the current library without changing
the shared source. Its behavior test failed with exit 101. The existing 206-test
focused run independently passed, demonstrating the regression suite's gap.

Expanded reproduction confirmed repeated separators and embedded dot components
in confined mode. Host mode additionally accepts leading dot components,
absolute workspace paths and a symlink alias. All seven accepted aliases skipped
the gate and overwrote the same target; canonical spelling was denied in both
modes. Confined mode rejects leading-dot, absolute and symlink variants.

Required correction: matcher path identity must agree with the actual admitted
workspace-relative path, or aliases must be rejected before effects. Cover
equivalent spellings with actual denied writes and a permitted control.

Correction submitted: resolve accepted paths through the existing descriptor
admission policy before matcher selection and after rewrites. Pin the existing
target or new file's parent, bind the actual opened descriptor to that identity,
and recheck its path after the final awaited scan. Original retry arguments stay
immutable. The new integration matrix covers 52 denied/permitted alias cases;
additional tests cover rewritten aliases, missing parents, and target changes
during gate waits and the final scan. These passed in the corrected candidate's
250-test run. Independent reproduction and source re-review passed as recorded above.

### S2 — P2: Concurrent uncertainty discards a completed deny

Closed by independent re-review. The original `finish_plugin_hook` required an active owner when settling an existing
invocation. In a completed concurrent group, the first transport failure sets
`recovery_pending`. Saving the next handler's completed deny then fails, leaving
its outcome unknown and its questions empty. The tool remains held, but the
retained history loses a known deny. The independent probe reproduced this at
`src/workflow/runtime/plugin_admission.rs:222` and `:260`, with the result loop
at `src/plugins/admission.rs:335`.

Required correction: settle already-reserved invocations after sibling
uncertainty without granting further execution. Preserve every completed deny
and retain the owner hold. Add failure/deny ordering and unchanged controls.

Correction submitted: settlement validates an existing reserved invocation and
its immutable identity without requiring authority to start new work. Known
results remain writable after sibling recovery or an owner hold; new execution
still rejects both. Both-order failure/deny/allow tests and owner-hold tests pass.
Changed identities, duplicate settlements and unknown replacement outcomes are
rejected without partial record mutation. Independent re-review passed as recorded above.
