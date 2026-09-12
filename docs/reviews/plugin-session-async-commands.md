# Native asynchronous session commands

Status: verified bounded prerequisite. This work belongs to the active
lifecycle-dispatch item in
the complete 41-requirement skills/plugins/hooks commitment.

## Behavior and ownership

The [decision](../decisions/own-native-asynchronous-session-commands-with-their-original-grant-and-host-lifetime.md)
enables explicitly declared native async Command observations under their original
SessionHooks time grant and actual native host. Synchronous commands remain
grant-free. Commands and their context spend no model, backend or Agent inspection
slots. Native asyncRewake stays rejected; Claude-only first-line transfer and
ordinary task/child continuation retain their own source semantics.

First transfer requires the original unsettled occurrence and must be durable
before startup releases readiness. After transfer, the exact pending hook, immutable
admission/declaration/source/once identity, original BudgetRef, durable receipt and
live host authorize effects. Settled startup alone does not revoke transferred
work. Pinned workspace and effective protected roots remain checked while permitted
command writes stay valid. Task operations cannot fund, replace or cancel this
independent session owner. Same-workspace writers quiesce before snapshots; child
workspace selection remains scoped.

Final termination revokes startup jobs and withholds their context. Unknown effects
retain recovery holds and can prevent terminal effects. SessionEnd commands share
the original two-second observation window, followed by three seconds of actual
cleanup. The five-second whole end boundary, eight-second application reservation
and thirty-second startup boundary remain. Abort/drop and interrupted startup own
process and descendant teardown even when runtime/event handles survive.

Startup context is attributed and bounded to 64 KiB. It can join only an already
admitted native Creator request, after begin_model, with its exact delivery target:

- Before append, revoked optional context is durably withheld; an independently
  valid ordinary request can proceed without it.
- After append/checkpoint, authority loss retains partial-delivery history and a
  recovery hold. The Model API has no rollback; invalid context is not sent.
- After a valid request was sent, a known successful response can settle its
  original target after grant expiry or cancellation. It cannot settle a replaced
  hook or renew execution authority.

Completion cannot create a request, enter the post-response correction branch,
select or mutate a task, invoke developer controls, or provide terminal context.
Hook models, children, reviewers and external owners cannot borrow native context.
Launch and transfer do not consume once. Exact success does; failed work remains
eligible later; unknown work stays held and does not replay on restore. Eight
active jobs and 64 pending deliveries remain bounded; cleanup owns permit release.

## Independent review findings

The first SPEC review failed three boundaries. Compiled controls reproduced each
before repair. The second review confirms those repairs and the additional delivery
identity repair, but finds a first-line publication race.

| Finding | Observed consequence and disposition |
| --- | --- |
| F1: first transfer used relaxed live-host validation | Expired, closed or settled startup could acquire its first transfer. Repair requires the original occurrence before transfer and preserves valid continuation afterward. |
| F2: durable receipt was optional or incompletely compared | Effects could proceed after receipt removal or owner/task/allocation/deadline/status/marker/target substitution. Repair captures and compares the exact transfer. |
| F3: old success used current replaced hook identity | Changed activation, declaration or admission could become Succeeded. Repair leaves that replacement untouched and recovery held; original known outcomes after simple expiry remain recordable. |
| Additional context completion identity | A replacement retaining the same target could become Delivered. Retaining the captured immutable identity prevents that settlement without rejecting original late responses. |
| Absolute teardown assertion | The old test allowed an extra cleanup wait after host return. The corrected real host test checks one original ended+5 deadline. |
| F4: concurrent first-line publication | A durable receipt became visible before local identity publication, so the actual monitor could reject legitimate transfer. The repair requires matching captured intent and durable receipt under the runtime lock, while keeping scheduling and cleanup ownership distinct. Its focused controls pass, and independent rereview confirms the repair. |

The F4 control uses an actual Claude declaration, production dispatch and monitor,
and a controlled blocking first-line callback paused at the publication seam.
Existing packaged Claude regressions cover the actual parser/process path. Failed
persistence must still block effects; local intent is not durable authorization.
Each finding was recorded here before its separate implementation repair.

Archived independent reports under `/home/shawn/demoncoder-check-tmp/`:

- `session-async-commands-spec-review-fail-20260912T181035550155Z.md`, SHA256
  `06e593aa1b9b831907812c823c3b3692efed5dbe55a316b47025693c7d3a3238`.
- `session-async-commands-spec-review-fail-20260912T182431273785Z.md`, SHA256
  `587661c62e42423a8b1cf9742473690c3b211565865c457f6ec020f339efb922`.

The third SPEC review passes the current frozen source with no remaining source
finding. Its report is `session-async-commands-spec-review.md`, SHA256
`7fa62fdb684768b8c089959a21b833bbae5c608e87d9928f967081de87a6f5d2`.
It was issued before the full rerun completed and retains that timing explicitly.
The later full receipt and final QUALITY audit close that verification step.
QUALITY approves this exact candidate in `session-async-commands-quality-review.md`,
SHA256 `19baac04a5b3ddc714c8b6c34248db4346d4fbbd4e4bbb854c77b0db5d70a3f9`.
Neither bounded review approves complete product delivery.

## Candidate verification

Baseline: `881e29d9b07eff8f22c5205aac74f2ff76b0c3b7`. The earlier 964-test transport
prerequisite does not verify this async change. All artifact names below use the
`session-async-commands-` prefix in `/home/shawn/demoncoder-check-tmp/`.

| Candidate or check | Actual result |
| --- | --- |
| First all-targets run | 402 library passes and one obsolete diagnostic expectation failure; its preceding zero-call and zero-task-debit assertions passed. First Clippy also found a collapsible condition. Both were corrected in an announced thaw. |
| Freeze `180116806898Z` | 985 passes, zero failures, 17 ignored, 45 suites; exit zero in `all-targets-corrected-20260912T180117129987Z`. Raw log SHA256 `6d619526f615c5b01890e8531a2fe7b2eb9111fa0b5b6aabb99fd6b416d3f4f6`. First SPEC nevertheless failed F1–F3. |
| Freeze `182006105258Z` | 989 passes, zero failures, 17 ignored, 45 suites; exit zero in `all-targets-spec-repaired-20260912T182013065117Z`. Raw log SHA256 `e8dd24b8145ca57e6c62bfe680298c7a86af645c4c328feb0a2f20a3881fa54b`. Second SPEC nevertheless failed F4. |
| Clippy on the second reviewed source | Passed `clippy-spec-repair-20260912T181934178898Z`. |
| Separate terminal check on that binary | Three cases passed in `parent-pty-20260912T182118562310Z`; binary unchanged before/after. This pass becomes historical after the F4 rebuild. |
| Publication repair freeze `183607112712Z` | 991 passes, zero failures, 17 ignored, 45 suites in the unchanged full rerun; exit zero. Focused race control, 14 native cases, Clippy, format, diff, three terminal cases and SPEC also pass. QUALITY approves; parent and reviewer independently verify all indexed hashes. |

Parent independently parsed both completed full-run counts and exits. The second
reviewed manifest has SHA256
`31c56f8e12002d2fad570a04fec32af2160a48f25956c39eb429b722182b0d7b`.
All 356 repository and four external inputs matched in
`parent-freeze3-audit.json`; the worker checked them again before the F4 thaw.
The prior manifest SHA256 was
`4a4b9326fe050c47f537f1b30294779abd11653095aa1547be82dfc87b59a69c`.

The historical terminal binary SHA256 is
`914ea972d69ceb58579e53eaf2ddcc235190cedf0a5166853bf551f9808078b6`;
its raw terminal log SHA256 is
`b98a95ffb66f072f24cc0acb5602880e7e0b413b51b2970b967a5dccdb372000`.
No obsolete pass verifies a later source revision.

The current source manifest is `source-freeze-20260912T183607112712Z.json`, SHA256
`c2f5747ce3747ac88b7a0cd0b9c41583292dfeea2d5ac60974598e96da0a48e3`.
Parent independently checked all 356 repository and four external inputs with
zero mismatches in `parent-freeze4-audit.json`. Current main/test binary identities
are in `publication-build-identities.json`; the previous identity receipt remains
unchanged. Clippy passed in `clippy-publication-final-20260912T183544884275Z`.

The first full run on this freeze, `all-targets-publication-final-20260912T183616110421Z`,
passed 409 library cases but later exited 101. Two developer-access tests failed
during bwrap mount setup for `/home/shawn/.claude.json`, before their assertions;
that target recorded 13 passes, two failures and one ignored case. Parent inspected
the raw errors. The unchanged targeted rerun,
`developer-access-mount-rerun-20260912T183900145678Z`, passed 15 with one ignored.
All frozen inputs still match. The cause of the mount failure remains unknown;
only safe file metadata was inspected, and no source or environment was changed.
The failed run is retained. The unchanged full rerun,
`all-targets-publication-stable-rerun-20260912T183906709733Z`, passed all 991 tests
with 17 ignored across 45 suites. Parent independently parsed the raw summaries
and exit zero in `parent-current-run-audit.json`. Raw log SHA256:
`6590f4b9efab18783c289ef0251e49e7727bcb4e1b28384999be436e441fdef7`.

The current terminal check passed three cases in
`parent-pty-20260912T183737244917Z`. Its binary SHA256 remained unchanged:
`ab0623ded2530b53610712060a0d227dd6eb1778161c036e4b253392371864d8`.
Raw terminal log SHA256:
`e9866010b492cb4843b1194cdadda8bd2c82d04f1c1632976284617774e717e9`.

A synthetic temporary-file probe reproduced the mount error class in 19 of 20
replacement attempts; five stable-target controls passed. This demonstrates the
existing live destination dependency, not the exact cause of the two original
failures. Real private configuration was not read or changed. The narrow source
trace and probes are retained with a [captured follow-up](../../.cairn/backlog/investigate-intermittent-private-settings-mount-setup-failures.md).

The closed worker index is `final-evidence-index-20260912T185402342940Z.json`,
SHA256 `b1353f9ec5c521c286138db320fad25bc90bee57a03e0db61ea430f0a3b91d2a`.
Parent and QUALITY each independently verified its 234 artifacts, 46 binaries and
all 360 frozen inputs, with zero mismatches. The parent consolidated manifest adds
final review publications, its audits and the handoff: 246 artifacts in
`evidence-manifest-final.json`, SHA256
`6b62174c56397f1257745260bb0d3a05cf5ae8bb018cf72a1b1531283a5d15bd`.
Mutable report publications are bound separately, without circular hashes.

## Compiled controls and failure classification

The implementer's `coverage.md` and final evidence index retain the full development
history. Parent inspected the raw results named here; the independent reviewer also
checked all four stable intentional variants and their archives/restoration.

| Control | Actual observation |
| --- | --- |
| `red-host-corrected-20260912T170948958014Z` | Four compiled baseline failures show absent transfer, terminal effect and context behavior. Intermediate host cases subsequently passed; the expanded host suite has 11 cases. |
| `red-epoch-20260912T171015222458Z` | Ordinary context incorrectly survived budget replacement with equal clocks/limits but a new epoch. `green-epoch-correct-filter-20260912T172540358729Z` ran and passed one corrected case. The repair stays at the same original-owner boundary. |
| `red-optional-context-boundary-20260912T173053154896Z` | Pre-append revocation incorrectly failed an ordinary request. `green-optional-context-boundary-20260912T173137715832Z` passed all three boundary cases. |
| `red-spec-owner-boundaries-compiled-20260912T181442947139Z` | Three assertion groups reproduced F1–F3. `green-spec-owner-boundaries-20260912T181618768473Z` passed 12 native cases. |
| `red-spec-delivery-replacement-20260912T181821457012Z` | Known delivery incorrectly settled a replacement with the same target. `green-spec-delivery-replacement-20260912T181911589132Z` passed all 13 native cases. |
| `green-spec-terminal-and-host-20260912T181658170941Z` | All 11 packaged host cases passed, including the corrected absolute teardown deadline. |
| `red-first-line-publication-joined-20260912T183323437223Z` | The production monitor rejected a valid durable receipt in the publication gap. The callback was released and joined before the assertion. `green-first-line-publication-20260912T183453827101Z` passes that control, and `green-publication-native-regressions-20260912T183520378835Z` passes all 14 native cases, including failed persistence. |
| Stable transfer-cleanup variant `175456926245Z` | Removing abandonment cleanup lost the uncertain durable outcome; the unchanged test failed. |
| Stable checkpoint-send variant `175054828581Z` | Removing final validation allowed the revoked-context turn to complete; the unchanged test failed. |
| Stable terminal-join variant `175324301013Z` | Removing the terminal join returned without the expected terminal effect; the unchanged test failed after cleanup. |
| Stable writer-quiescence variant `175407579412Z` | Removing native writer selection admitted a snapshot while its writer waited; the unchanged test failed. |

Each stable variant retains its one-file patch, all 309 original inputs, before/after
manifests and exact restoration. The real-process terminal control finishes cleanup
before asserting; the others use bounded controlled runners without descendants.

These earlier failures are retained but excluded from behavioral proof:

- Private helper access, TurnEnd Debug, Box/type and enum-name errors failed
  compilation. One exact filter selected zero tests; it is not a passing control.
- First-line fixture `183132448733Z` lacked the required Claude concurrent-group
  declaration. Run `183226471992Z` detected the premature result but asserted before
  joining its callback; it is an inadequate harness run. The joined rerun supplies
  the retained behavioral proof, with `first-line-red-joined-inputs` archive/manifest.
- Several fixtures omitted frozen source/once metadata or tried End before settling
  startup. A second SessionStart correctly hit replay refusal.
- An early authority fixture changed the settled startup deadline instead of the
  actual host. Legitimate transferred work survives settled startup.
- An unconfigured default `.env` alias did not change effective credential policy.
  The corrected fixture configures an external alias in both captured workspace and
  ToolExecutor, then remaps its target; that actual authority change denies effects.
- `violating-checkpoint-send-20260912T174852015684Z` overlapped compilation with source
  edits. Its candidate is indeterminate and cannot establish a stable falsifier.
- Scratch runner `mutant-checkpoint-send-20260912T175017197206Z` shadowed its target
  variable and overwrote Cargo.lock. Cargo failed before compilation. The original
  failed-restoration report remains unchanged, with a separate repair receipt.
  Parent verified all 309 original inputs against archive and restored worktree,
  and checked Cargo.lock equals HEAD. The corrected runner passed a temporary
  archive/mutate/simulated-failure/restore self-test before more variants.

Cargo.lock SHA256 after that repair is
`8e7fffb0e2dde3bbc1c453cc99a912a0f3f2b56f5ff03d82f9bcac357a6b76d7`;
parent receipt: `parent-repair-audit-20260912T175249422664Z.json`. Cargo config,
dependencies and ordinary accounting were not changed by this task.

## Static diagnostics and verification limits

Current Ripwire quality-delta returned 2 with 157 rows, including 70 gating rows on
existing symbols. Test-gate returned 4 with 51 test files and 533 graph-unmapped
symbols. These are nonpasses, not failed executed tests. The final reports are
`quality-delta-publication-final-20260912T183641157215Z` and
`test-gate-publication-final-20260912T183648712545Z`; the transfer edit check passed
in `edit-check-publication-final-20260912T183732229745Z`.
The static review distinguishes real complexity/churn costs from Rust trait/name
attribution gaps and records source-based dispositions without suppressions or
baseline changes. Independent QUALITY confirms those source-based dispositions
and accepts the explicit maintenance costs without claiming clean static scores.

Direct real command controls cover task expiry and replacement. Cancellation,
acceptance and archive independence use source audit plus existing regressions;
they are not three additional real-process scenarios. Native once/recovery uses
controlled runners, serde and production interrupt_restored, not a new real-process
reopen qualification. Existing actual command/restart regressions remain required.
Native pending/pruning uses 31 additional handlers under the real 32-hook occurrence
cap; the ordinary effect-65 regression covers the shared 64-pending limit.

Controlled native cases do not refresh installed/live backend qualification.
Public activation, source conversion, external lifecycle qualification, all package
components and full conformance remain part of the same open commitment. Final
source verification, both independent approvals and the parent's 14-rule audit
are complete for this prerequisite. The audit is retained in
`parent-self-audit.md` and bound by the consolidated manifest. Development tests
are not Cairn receipts.
