# Settings publication and ConfigChange

Status: The bounded ConfigChange prerequisite is implemented and verified.
Independent specification and quality reviews approve the final candidate.
The complete lifecycle and 41-requirement commitment remain open. Aggregate
static checks remain nonpasses, with their disposition recorded below.

The [decision](../decisions/gate-actual-settings-publication-with-the-original-host-session-policy.md)
adds ConfigChange at the existing Settings save boundary. The current active
policy judges the exact proposed change before private atomic publication.
This is one prerequisite within complete lifecycle dispatch; the full skills,
plugins and hooks commitment remains open.

## Existing behavior and implementation risks

The existing save already checks a draft's original disk revision, validates its
configuration, takes the active-state mutex and publishes through the private
settings store. A failure after rename marks publication uncertain and prevents
new work from assuming the old settings remain active. A gate must preserve that
ordering while inspecting outside the mutex and revalidating before publication.

Before this change, the Settings panel had no lifecycle owner. Although the runtime creates
an explicit session allowance for every connection, its executable host lifetime
was opened only for native sessions. A settings control needs original
session authority on all four connections without fabricating native or external
source events, borrowing an unrelated running task's budget, or resetting its
grant when a later model replacement changes provider identity.

The source map was read independently before implementation. This review must
challenge old-policy selection, source applicability, private configuration
exposure, exact draft/input/owner validation, cancellation during active work,
receipt failure after publication, and later provider replacement. It must retain
actual violating examples and corrected outcomes, including their proof limits.

## Ordinary live Settings baseline

The existing production terminal suite `python3 tests/live_settings.py` passed
all six cases in 3.440 seconds before this implementation. It exercised held
native and external turns, later eligible defaults, explicit launch overrides,
cancelled discovery, provider deselection, and conflicting or failed saves.
It used the retained application from the preceding verified prerequisite,
SHA-256 `d90cfb1aba05eba6323d8f7343b4ecdcc3c8acf16c80494b302fb199efb14c86`,
against controlled provider/backend fixtures. It does not establish plugin-gated
saves or live-provider behavior.

The raw command, output and exit status have prefix
`/home/shawn/demoncoder-check-tmp/lifecycle-config-change-baseline-live-settings-20260912T235836613630Z`.
Its source and artifact binding is
`/home/shawn/demoncoder-check-tmp/lifecycle-config-change-baseline-binding-20260912T235836613630Z.json`,
SHA-256 `5ddbdeb64f87fc89a79f13acfeb73161111324a8b5eb0405dcdfca5bb0b05270`.
The baseline Git commit is `ceaae3391bd8508c436edb1cfa1b281d3a8f93f3`.

## Initial behavioral failures

The focused plan-admission test reached the existing event restriction and failed
because ConfigChange was not accepted as an owned non-tool event. Its raw prefix is
`lifecycle-config-change-red-non-tool-config-change-behavior-20260913T001236794381Z`.
An earlier command used an unqualified name with `--exact`; it selected no test
and is not a failure demonstration. The corrected admission check then passed in
`lifecycle-config-change-green-non-tool-config-change-20260913T001247834420Z`.

The production Settings test compiled and executed `Handle::save` with a denying
active ConfigChange policy. The save returned success, failing the required
refusal assertion. This is a behavioral failure at the actual publication API,
not a compilation or fixture setup failure. The raw command and output are retained
under `lifecycle-config-change-red-handle-denied-save-20260913T001404454163Z`.
The first correction reached dispatch but stopped with `invalid post-tool owner
event: ConfigChange`. The typed non-tool runner-owner and cancellation settlement
routes were missing ConfigChange, so it fell through to post-tool ownership.
The denial test deliberately requires the hook's actual reason; an unrelated
early hold cannot satisfy it.

After correcting those typed routes, the exact locked library test passed once in
`lifecycle-config-change-green-handle-denied-save-lib-locked-20260913T003507167819Z`.
It checks the specific hook denial, unchanged saved bytes and the unchanged active
Creator model. That case uses an in-memory handler registered as Command; it does
not establish command-process execution. Earlier compilation
and ownership failures remain retained; no broader test pass is inferred.

## Production-runner coverage in progress

Private Settings APIs remain private. Minimal runner fixture support is compiled
only for the library's actual save tests. Each
applicable runner must execute through an actual Settings save: native command,
HTTP, MCP, prompt and agent, plus Claude command, HTTP and MCP. A schema check or
an in-memory runner with a matching kind is not delivery evidence.

The first real command-runner save case failed its exact denial-reason assertion
in `lifecycle-config-change-red-production-command-actual-save-20260913T005352764233Z`.
The retained follow-up diagnostic identified a missing fixture supervisor:
`lifecycle-config-change-lifecycle-config-change-red-production-command-diagnostic-2-20260913T010417448815Z`.
It had not reached the command process. After configuring the supervisor and
separating ConfigChange eligibility from native-only startup observations, the
actual command-runner test passed in
`lifecycle-config-change-lifecycle-config-change-production-command-real-20260913T010908691389Z`.
Its one test executes four cases: Native and Claude each allow and deny a real
save. The confined script checks ConfigChange, truthful user_settings attribution,
and absence of the private API-key canary. The denied save retains the old file
and active state. This pass alone does not qualify the whole prerequisite.

The next focused passes exercised real HTTP and managed MCP calls for both
Native and Claude, followed by real OpenAI-compatible model requests for native
Prompt and Agent. Each test executed its four allow/deny cases and checked actual
saved state; peers asserted the event, source and private-value exclusions.
Raw prefixes are:

- `lifecycle-config-change-lifecycle-config-change-red-production-http-real-2-20260913T011422663738Z`
- `lifecycle-config-change-lifecycle-config-change-red-production-mcp-real-20260913T011442771873Z`
- `lifecycle-config-change-lifecycle-config-change-red-production-model-real-2-20260913T011708106334Z`

Those historical filenames contain `red`, but their recorded outcomes are passes.
The preceding model invocation selected zero tests because the test was nested;
`lifecycle-config-change-lifecycle-config-change-red-production-model-real-20260913T011623574678Z`
is retained as an invalid verification attempt. These controlled peers do not
establish live-provider behavior or the full safety/recovery matrix.

The combined run with accounting assertions passed seven tests, including all
four Settings tests, in
`lifecycle-config-change-lifecycle-config-change-production-all-accounting-20260913T012241630617Z`.
A narrower run selected the four Settings tests and passed all 16 allow/deny
cases in
`lifecycle-config-change-lifecycle-config-change-production-four-accounting-20260913T012307681785Z`.
The assertions bind ConfigChange and its funded suboperations to the original
host session, require no task or child owner, check actual model/service operation
counts and cumulative calls/token usage, and verify unchanged allowance start
and deadline. Active unrelated tasks, exhaustion and provider replacement remain
separate outstanding cases.

Parent inspection found an unnecessary integration test that only asserted the
runner kind returned by a fixture factory. Its module import also suppressed
dead-code warnings for the unused transport fixtures. Both were removed. That
test's historical pass establishes no runner execution. The real save matrix
continues to exercise every fixture through the private library test seam; final
lib/tests Clippy and formatting passed after the removal.

The final bounded runner set passed four tests and all 16 cases after strengthening
the delivered-frame assertions, in
`lifecycle-config-change-lifecycle-config-change-production-four-exact-frames-4-20260913T013243913675Z`.
Claude Command, HTTP and MCP each require the six real ConfigChange fields;
native MCP retains its declared native binding. Earlier attempts exposed generated
Python fixture syntax and MCP frame-expectation errors; their raw results remain
retained. Final lib/tests Clippy passed with `-D warnings` in
`lifecycle-config-change-lifecycle-config-change-clippy-final-3-20260913T013255428433Z`,
and formatting passed. Focused ConfigChange and actual native session-lifetime
regressions passed after the dispatcher helper refactor. The bounded handoff is
`/home/shawn/demoncoder-check-tmp/config-change-real-runners-handoff.md`.

Ripwire's quality delta still exits 2 against the shared dirty tree, reporting
45 major findings across that tree. The dispatcher delta is now a minor increase
from 73 to 74. Its test gate exits 4. These are review inputs, not passing checks;
the full prerequisite has not entered independent review or full regression yet.

An early development application passed all six ordinary live Settings terminal
cases in 4.019 seconds, with raw prefix
`lifecycle-config-change-early-live-settings-20260913T005032191377Z`.
The retained executable is `lifecycle-config-change-early-app-20260913T004825284244Z`,
SHA-256 `1b9add79cdfb93b22cfbff7d5d476c54824f92838657d5a355983cfcb53f9f88`;
its companion JSON binds the copied executable and build receipt. These controlled
peer cases predate the later runner fixture changes. They establish ordinary
Settings behavior for that early executable, not final qualification or plugin
delivery.

## Publication safety development checks

The first stale-input controls passed two private Settings API tests in
`lifecycle-config-change-publication-safety-stale-controls-1-20260913T014900094565Z`.
They change an inspected workspace file or the settings file while a held
in-memory handler is running. The actual save refuses the stale pass, retains
the original active model and one unpublished invocation, and calls the handler
once. These tests establish publication behavior; their handler is not a process
or transport runner.

Three initial failure controls passed in
`lifecycle-config-change-publication-safety-real-failures-1-20260913T014919840257Z`.
An actual pre-publication permission failure preserves the original settings
bytes and active model. An instance-scoped test fault after the real settings
rename produces `AppliedUncertain`, retains the new file and holds the current
Handle. A workflow receipt directory-sync fault after actual settings publication
holds both runtime and Handle; reopening retains the operation and does not call
the in-memory handler again. The directory-sync fault leaves the new receipt
visible; it does not prove recovery when the old receipt remains on disk.

The distinct receipt pre-rename case subsequently passed alongside the directory
sync case in
`lifecycle-config-change-publication-safety-receipt-boundaries-1-20260913T015152813623Z`.
After settings publication, this fault leaves the older settled gate receipt on
disk without a publication outcome. Reopening retains that exact operation and
absence of recorded publication, loads the saved settings, and does not repeat
the handler. It does not retroactively manufacture a publication receipt.

The then-current Settings store suite passed all 21 tests in
`lifecycle-config-change-publication-safety-store-suite-final-2-20260913T015420091055Z`,
including the 16 real runner/source cases. A separate uncertainty-formatting test
passed in `lifecycle-config-change-publication-safety-ui-uncertain-20260913T015525041655Z`;
that formatter test does not establish actual terminal cancellation or display.

The first blocking-publication abort control passed in
`lifecycle-config-change-publication-safety-aborted-waiter-20260913T020237241163Z`.
It pauses after actual Settings replacement and before the workflow receipt write,
aborts the awaiting save, and verifies the workspace mutation lock remains held.
The blocking transaction then encounters the injected receipt pre-rename failure.
New settings bytes remain, Handle/runtime hold, and reopening retains the older
settled gate receipt without replay. This proves post-publication transaction
ownership; it does not prove process cleanup or cancellation before admission.

Explicit pre-admission revocation passed in
`lifecycle-config-change-publication-safety-revoked-before-admission-20260913T020709966229Z`.
The awaiting save retains the attempt's cancellation guard; the blocking worker
owns the workspace lock. Cancelling before final runtime admission therefore
revokes publication without detaching its transaction resources.

The competing-save control initially blocked its single-threaded test executor
on the production policy mutex. Adding a second executor thread was insufficient
proof of responsiveness. The production save now tries policy capture without
waiting behind an in-progress publication, and the final single-threaded control
requires the second save to return within 100 ms while the first is paused after
rename. The first saved revision remains readable without a stale uncertainty
flag. This passed in
`lifecycle-config-change-publication-safety-concurrent-responsive-final-20260913T020952989446Z`.
Final transactions serialize transient uncertainty per Handle, including ordinary
no-policy saves.

All 24 focused Settings tests passed in
`lifecycle-config-change-publication-safety-store-suite-final-4-20260913T021023359916Z`.
These development results do not close the ConfigChange prerequisite; actual
Panel/process cleanup, the four-owner matrix and independent reviews remain.

## Cancellation development checks

The actual command-descendant control failed before the cleanup change in
`lifecycle-config-change-cancellation-red-app-command-20260913T022214397380Z`:
the session shutdown path returned while Settings command descendants remained
alive. The test recorded that observation, then stopped the fixture before
asserting failure. The same test passed after the change in
`lifecycle-config-change-cancellation-green-app-command-20260913T022433904918Z`.

This first case checks real process lifetime through the session command/worker
path. An unmodified draft or an async job's completion alone cannot establish
publication refusal or whole-application timing.

The first actual Panel Escape case passed in
`lifecycle-config-change-cancellation-panel-command-final-20260913T023333950644Z`.
It cancels a changed draft through the real Escape handler, polls until the panel
closes, checks that command descendants have stopped, and verifies the original
settings bytes/model and an unpublished ConfigChange receipt. This version inserts
the private save job into the Panel directly; it does not yet establish the Save
key path. Earlier runs include a fixture compile error, a missing fixture path,
and an incorrect expectation that an interrupted gate had settled. The final
assertion preserves the unpublished receipt without calling interruption a pass.

The later Panel suite passed four tests in
`lifecycle-config-change-cancellation-panel-suite-2-20260913T024010220722Z`.
Its command case now starts the save through actual editor navigation/Save keys
and a controlled model catalog response, then uses Escape and checks descendant
exit and unchanged disk/active settings. Two additional cases cancel actual MCP
execution: a pending stdio call with a child process, and a pending HTTP call whose
late reply is withheld. The stdio case checks process exit before Panel closure;
the HTTP case retains one call and unchanged settings after the delayed reply.
Those two cases enter the private save job directly and exercise the real Escape
and polling path. The fourth test checks uncertainty wording only.

Mid-implementation review found that awaiting runner cleanup inside Panel polling
would block keyboard/render handling, and awaiting a full gate deadline before
the main shutdown call would add an extra application shutdown budget. The final
bounded implementation moves save and drain into a background completion job.
Panel polling consumes only a finished job. Terminal exit synchronously revokes
the attempt and returns to the existing application shutdown boundary.

The synthetic save-task failure cleanup control passed in
`lifecycle-config-change-cancellation-task-failure-cleanup-2-20260913T024719814747Z`.
Its deliberate Rust panic is test input; the test completed successfully. This is
distinct from an unexplained native crash or a process-runner execution result.

The first timeout cleanup case passed in
`lifecycle-config-change-cancellation-timeout-cleanup-3-20260913T024824424536Z`,
but a stronger diagnostic check failed in
`lifecycle-config-change-cancellation-timeout-cleanup-diagnostic-20260913T024906101213Z`.
That failure concerns the displayed reason: it reported a held/settled lifecycle
owner rather than deadline expiry. The assertion ran before the final process
check, so this diagnostic run does not establish an early process-close bug.
The final exact-deadline case instead holds a real HTTP MCP call whose ten-second
service timeout exceeds the two-second Settings deadline. It checks the explicit
Settings deadline reason, unchanged publication and completed local call cleanup.
The execution deadline is unchanged; cleanup has the existing five-second
reservation. This is separate from the application's existing shutdown deadline.

An intermediate unrelated-task assertion compared a task before and after its
ordinary IdleModel turn completed. The change from stopped=false to stopped=true
was normal completion, not demonstrated Settings cancellation. The corrected
stable-task test waits for that turn to finish before taking its baseline. It
establishes retained task state and allocation, not preservation of an in-flight
turn. The actual four-connection task fixture must still hold a provider request,
cancel Settings through Panel Escape, and release the unrelated turn to complete
normally. This case remains required before ConfigChange approval.

Final cancellation development checks passed:

- `lifecycle-config-change-cancellation-store-suite-final-20260913T030043642490Z`:
  27 Settings tests, including the preceding sixteen actual runner/source cases,
  publication faults/races, pre-operation cancellation and panic cleanup.
- `lifecycle-config-change-cancellation-panel-suite-final-4-20260913T030528354039Z`:
  six Panel tests, including the exact pending-MCP deadline and terminal-exit
  primitive followed by the external application shutdown wrapper.
- `lifecycle-config-change-cancellation-session-regressions-20260913T030026443596Z`:
  six session and native-lifetime regressions, including strict stale-identity
  rejection and retained cumulative funding.
- `lifecycle-config-change-cancellation-clippy-lib-tests-3-20260913T030545711737Z`:
  library/test Clippy with warnings denied. Final formatting and diff checks passed.

The parent read the raw successful test logs and the background job/drain code.
Inner save-task failure still reaches the outer cleanup owner; failure of that
outer owner reports cleanup uncertainty. Operation-scoped drain uses the existing
runner leases and validates the ConfigChange receipt's exact operation and host
lifetime. It does not equate an idle retained MCP connection with an active call.
The terminal-exit behavioral case exercises the Panel exit primitive and actual
application wrapper; terminal input EOF/error variants are inspected common-path
coverage, not individually injected terminal failures. The four actual connection
owners remain the next piece.

Cancellation's final shared-tree Ripwire checks remain nonpassing: quality-delta
exits 2 with 73 major findings, and test-gate exits 4 with 53 broad test files and
620 reported untested impacted symbols. These are static reports, not additional
passing checks. Final review must reconcile them with the full regression and
source inspection; no suppressions or unrelated refactors were added.

## Actual connection-owner work in progress

The stable-task attribution control failed in
`lifecycle-config-change-owner-matrix-red-task-owner-20260913T031544250486Z`:
the Settings operation recorded the unrelated coding task as its owner
(`Some(1)` rather than `None`). The corrected control passed in
`lifecycle-config-change-owner-matrix-green-task-owner-20260913T031607168426Z`.
The parent read both raw logs. This establishes an attribution failure and its
bounded regression; it does not by itself establish mischarged funds, four actual
connection owners or preservation of an in-flight turn. Those cases and retained
MCP behavior across Creator replacement remain under implementation.

The subsequent actual-owner fixtures passed:

- `lifecycle-config-change-owner-matrix-api-owners-corrected-20260913T032323211110Z`:
  OpenAI and Anthropic built-in adapters against controlled peers.
- `lifecycle-config-change-owner-matrix-installed-owners-20260913T032343956777Z`:
  pinned installed Claude 2.1.267 and Codex 0.153.4 against controlled peers.
- `lifecycle-config-change-owner-matrix-panel-held-provider-20260913T032512661653Z`:
  an actual held OpenAI `/task` plus Panel Escape.

The first two tests hold ordinary provider turns during denied and allowed
Settings saves. They check original host-session funding and unchanged turn
identity without manufacturing workflow task/allocation state. The separate
OpenAI task case holds an actual provider request, starts a real command gate,
cancels it through Panel Escape, and checks that descendants have exited while
the task remains unstopped with the same selected task/allocation fields. After
releasing the peer it requires `TurnFinished` status `complete`. This case enters
the private save job directly; the earlier command fixture covers the editor's
Save key path. The parent read these three raw logs and the task case's assertions,
including the helper's explicit successful-completion check. This is controlled
transport/installed-backend evidence, not live-provider access.

The actual Prompt allowance case passed in
`lifecycle-config-change-owner-matrix-funding-final-20260913T033215895319Z`.
One funded save reaches the controlled model and retains its original grant start,
deadline and reported usage. A second save after call exhaustion reaches no extra
model request and preserves the first saved configuration. The visible hold is
currently the generic `required lifecycle handler failed`, with retained runner
reason `configured hook model request failed`; this is not an explicit explanation
that the session grant was exhausted. Earlier failed assertions expected more
specific wording. Final review must assess that diagnostic limit.

The real malformed HTTP reply case passed in
`lifecycle-config-change-owner-matrix-malformed-corrected-20260913T033440310574Z`.
Its diagnostic predecessor retained `HTTP hook returned invalid JSON or encoding`
on an incomplete, unpublished operation with uncertain handler effects. The
incorrect assertion expected another receipt state. This history is not evidence
that malformed output previously published settings. Temporary broad synthetic
operation debug output must not remain in the final fixture assertions.

These two development runs still warn about an as-yet unused replacement fixture
helper. They are test passes, not a current lint pass. Missing/expired funding and
actual Creator replacement with fresh and retained MCP are not yet approved.
Final source capture, independent review and broad verification have not yet run.

Retained MCP failed across actual Creator replacement in
`lifecycle-config-change-owner-matrix-red-retained-mcp-behavior-20260913T033816543910Z`:
the fresh Settings save after replacement did not apply. The same behavioral case
passed with explicit host-control service ownership in
`lifecycle-config-change-owner-matrix-green-retained-mcp-20260913T034017565176Z`.
The parent read both raw logs and the fixture: an actual initial turn, a default
save while the old Creator stays active, an eligible replacement turn, and a
second MCP-backed Settings save retain the original lifetime and grant. Explicit
initialize-count and peer-observed model assertions are being added to strengthen
the proof of retained reuse and actual model replacement.

The pending-at-replacement case then exposed a recovery interaction. In
`lifecycle-config-change-owner-matrix-replacement-recovery-diagnostic-20260913T034924210419Z`,
the old attempt was rejected, but a fresh save failed with `native session
observation requires reconciliation`. The parent read that failure excerpt and
`finish_phase`/`needs_reconciliation`: phase completion scans every non-delegated
incomplete operation, including a still-owned Settings gate. The repair must
distinguish owned Settings execution/cleanup and its causal admissions from
abandoned work, then finalize only safely known unpublished outcomes. It must
retain an invalidation hold, uncertain-effect and once/source-delivery obligations,
and every unrelated recovery cause. Merely clearing recovery afterward or treating
every ConfigChange as exempt would be unsound. This repair is in progress.

Further parent source review found the first recovery exception too broad:
`finish_phase` used any owned Settings occurrence to exempt all `settings` phase
operations. That could hide a separate abandoned occurrence or model admission.
The worker is replacing this with exact causal attribution and a mixed-owned/
abandoned negative control. The finalizer also needs exact receipt lifetime
binding, preserved holds and pending-delivery/causal-completion checks. No final
recovery approval is recorded yet.

The mixed-operation failure is reproduced in
`lifecycle-config-change-owner-matrix-red-mixed-settings-admission-20260913T041558461611Z`.
The parent read the raw failure: the live-owner/separate-abandoned-admission test
failed because `record.recovery_pending` remained false. Its corrected case is
still pending at that point.

The subsequent exact-causal suite passed 11 tests with its one explicit installed
case ignored in
`lifecycle-config-change-owner-matrix-exact-causal-suite-20260913T041830064941Z`.
The parent inspected the repaired source: `finish_phase` collects exact owned
ConfigChange IDs and checks typed HookModel/HookBackend/PluginService parents;
the finalizer checks the receipt's host lifetime, pending source delivery and
unfinished causal children before closing known unpublished work. Existing holds
and unrelated recovery flags are retained.

The actual delayed model-admission case passed in
`lifecycle-config-change-owner-matrix-settings-model-causal-isolated-20260913T042059305526Z`.
The parent read the raw passing result. Its intermediate failure reported changed
inspected inputs because the test peer's files changed inside the watched workspace;
isolating those fixture files preserves the required unchanged-workspace control.
This is not an additional production recovery defect. Final review remains pending.

The parent also inspected the blocking publication worker's join-error paths in
`Handle::publish_candidate`. A panic after file replacement leaves the sticky
uncertainty state but returns an ordinary error. An ungated save can reach the
Panel's `Not applied` notice; a gated save can instead show only cleanup failure
if the runtime lock was poisoned. The existing post-file-publication test pause
can inject this failure without a production fault switch. This is a source-review
finding awaiting its behavioral control and correction, not a reproduced failure
or a passing check. Pre-publication save-task panic coverage does not close it.

The publication worker reproduced both post-rename UI failures through actual
private file replacement, `start_save`, `SaveJob` and Panel polling. In
`lifecycle-config-change-publication-join-error-red-panel-post-rename-2-20260913T043317855656Z`,
the ungated save replaced the file, its worker panicked at the existing test pause,
and Escape incorrectly closed the Panel. In
`lifecycle-config-change-publication-join-error-red-panel-gated-post-rename-20260913T043337134146Z`,
the gated replacement followed by the same panic left only a cleanup notice,
omitting publication verification. The parent read both raw failing logs. The
first invocation of the ungated test had a test-source compile error; it is not
the behavioral failure. The correction now uses this save's transient publication
stage and a typed uncertain result, with no new durable ledger; verification is
still in progress.

Both corrected post-rename Panel cases pass in
`lifecycle-config-change-publication-join-error-green-panel-ungated-20260913T043621947974Z`
and `lifecycle-config-change-publication-join-error-green-panel-gated-20260913T043649407264Z`.
The parent read both raw results and inspected the per-save publication-stage
signal, its placement around the real save, and Panel handling of publication plus
cleanup uncertainty. Focused regression and independent review remain pending.

The parent then found a retry boundary still relying on changed disk bytes.
`Handle::current` checks sticky uncertainty, but draft capture and save admission
do not. When a save writes identical canonical bytes and directory synchronization
fails, an old draft can still match the disk revision on retry. A later clean save
can then clear the prior uncertainty. The publication worker is adding an actual
no-change failure/retry control and checking prior uncertainty before dispatch and
inside the serialized transaction. This remains a source finding until its
behavioral control runs; no stale-retry pass is claimed yet.

That retry failure is now reproduced in
`lifecycle-config-change-publication-join-error-red-same-draft-retry-20260913T044004386630Z`.
The parent read the raw result: the retry expected a refusal but returned
`Ok(Applied)` after the prior uncertain save. Its corrected case remains pending.

The corrected same-draft case passes in
`lifecycle-config-change-publication-join-error-green-same-draft-retry-20260913T044056006475Z`.
An already-inspected queued save is also refused at the publication transaction in
`lifecycle-config-change-publication-join-error-green-queued-uncertainty-20260913T044201872293Z`.
The separate concurrent-publication responsiveness check passes at
`lifecycle-config-change-publication-join-error-concurrent-responsive-20260913T044220711115Z`.
The final store suite passes 43 tests with one explicit installed case ignored in
`lifecycle-config-change-publication-join-error-store-suite-final-20260913T044224908929Z`;
Clippy lib/tests with warnings denied passes at
`lifecycle-config-change-publication-join-error-clippy-final-20260913T044231438424Z`.
The parent read the matching raw results. These close the reproduced retry fault
in focused development checks; final candidate binding and independent review
remain pending.

The owner/recovery worker completed its bounded implementation and handed off in
`/home/shawn/demoncoder-check-tmp/config-change-owner-matrix-handoff.md`.
Final development checks pass: owner matrix 13 (one installed case deliberately
ignored), the separate pinned installed case 1, Settings store 41 (one ignored),
Panel 8, runtime plugin regressions 95, tool operations 25, Clippy lib/tests with
warnings denied, formatting and diff checks. The handoff retains their exact raw
command/output prefixes. The post-rename JoinError issue remains open and is now
assigned back to the publication worker. No source freeze or independent approval
has been claimed.

Final owner-stage Ripwire reports remain nonpasses:
`lifecycle-config-change-owner-matrix-ripwire-quality-delta-20260913T042540255664Z`
exits 2 with 120 gating findings, and
`lifecycle-config-change-owner-matrix-ripwire-test-gate-20260913T042548866552Z`
exits 4 with 80 mapped tests and 611 untested symbols. Two focused edit-check
spellings failed symbol lookup. The parent retained a parsed review preparation in
scratch, without suppressing or dismissing findings; independent quality review
must inspect their concrete applicability. Shared fixture caller checks predate
the last Rust-only attribution repair and retain that candidate limitation.

The publication follow-up completed and released source/Cargo ownership. Its
durable handoff is
`/home/shawn/demoncoder-check-tmp/config-change-publication-join-error-handoff.md`.
Final Panel coverage passes 10 tests. The final source is awaiting a retained
snapshot and independent specification review. The latest whole-candidate static
reports are `lifecycle-config-change-publication-join-error-ripwire-quality-20260913T044306050614Z`
(exit 2, 122 gating findings) and
`lifecycle-config-change-publication-join-error-ripwire-test-gate-20260913T044312724143Z`
(exit 4, 80 mapped tests, 611 untested symbols). They remain nonpasses for review.

## Separate linker failure

`lifecycle-config-change-green-handle-denied-save-3-20260913T003105218409Z` failed
to link the unrelated `developer_access` test target. The linker reported an
undefined `core::array::try_from_trusted_iterator` symbol referenced by `ring`.
Its cause is not established. This is a build failure, not a Settings behavioral
result, and no global Cargo setting was changed.

The parent retained the implicated archive before any dependency cleanup at
`/home/shawn/demoncoder-check-tmp/lifecycle-config-change-ring-link-failure-20260913T003500545900Z.rlib`,
SHA-256 `5e934dcb3726c0567657d8046eabf7178174b2900f1f2a8e2952ce57cdd6c9c5`.
The companion JSON binds the raw failed command/output, compiler identity and
matching original/copy/original hashes. It preserves this archive, not every
linker input. A later read-only symbol inventory found both the reported reference
and a definition of that exact symbol in the retained archive; the original error
alone does not establish that the archive lacked the definition.

The exact target recheck, `cargo test --locked --test developer_access --no-run`,
passed in `lifecycle-config-change-developer-access-link-recheck-20260913T004015240328Z`.
No dependency cleanup, dependency rebuild or global Cargo change was performed.
This establishes that the current target links; it does not explain the preceding
failure or establish that the implementation source was unchanged between runs.

## Verification still required

The first independent specification review returned **SPEC FAIL** with one
concrete issue, F1: saves with Disabled policy discard the save attempt's
revocation authority, so immediate cancellation or cancellation of a queued
ungated worker can still publish. Initial setup exempts policy dispatch, not
ordinary Settings cancellation. The reviewer identified this by actual source
and UI control flow; it was not yet an executed failure. The remaining reviewed
narrow source and focused fixtures had no additional identified specification
defect. The complete review is retained at
`/home/shawn/demoncoder-check-tmp/config-change-independent-spec-review.md`.

That review is bound to all 2,827 inputs in
`lifecycle-config-change-source-freeze-20260913T044603839798Z.json` and its source
archive. Manifest SHA-256 is
`82f50f3d7ae537714d82a6b7b111d1f96d8218e2d63b631c68c04bc42de98dc3`;
archive SHA-256 is
`f70785c938582a5226fa1cedf73ce30c73ed382ffed9f30820f5636f9b7dbbb8`.
The exact-source comparator passed before review at
`lifecycle-config-change-source-freeze-verify-before-spec-20260913T044612897645Z`.
This accurately retained candidate is not approved. F1 is assigned back to the
publication worker for actual failure demonstration and correction, followed by
a new snapshot and the same independent reviewer's re-review. No full regression
or quality approval has been claimed.

F1 now has an executed failure at
`lifecycle-config-change-ungated-cancellation-red-immediate-20260913T045843812415Z`.
The parent read the raw result: cancelling the actual ungated `start_save` job
before it was polled still returned `Applied`. The queued Panel case and corrected
cases are in progress; independent re-review has not started.

The queued Panel Escape control also failed before correction at
`lifecycle-config-change-ungated-cancellation-red-queued-panel-20260913T045908316759Z`:
the file comparison showed replacement after pre-admission cancellation. Both
corrected controls pass at
`lifecycle-config-change-ungated-cancellation-green-immediate-20260913T050021937079Z`
and `lifecycle-config-change-ungated-cancellation-green-queued-panel-20260913T050039313573Z`.
The parent read the matching raw failures and passes. Every save now retains the
attempt guard across publication and checks revocation before dispatch and under
the publication transaction. No-policy saves gain no hook/runtime observations.
Final focused compatibility checks and independent re-review remain pending.

The corrected F1 Settings suite passes 44 tests with one explicit installed case
ignored at `lifecycle-config-change-ungated-cancellation-store-suite-20260913T050149393610Z`.
Panel coverage passes 11 tests at
`lifecycle-config-change-ungated-cancellation-panel-suite-20260913T050155965660Z`.
An additional actual queued no-policy exit/drop case passes at
`lifecycle-config-change-ungated-cancellation-green-queued-panel-exit-drop-20260913T050407335165Z`:
it revokes and drops the Panel/SaveJob while the worker owns the transaction but
has not admitted publication, then joins the transaction and verifies unchanged
disk/active state. The shared gated queued-worker control still passes at
`lifecycle-config-change-ungated-cancellation-gated-queued-helper-final-20260913T050426623851Z`.
Final Clippy with warnings denied passes at
`lifecycle-config-change-ungated-cancellation-clippy-lib-tests-final-20260913T050433328867Z`;
formatting and diff checks pass. The parent read the raw results and accepted
source/Cargo ownership back. F1 is ready for a new snapshot and targeted
independent re-review; no SPEC PASS or full-regression result is claimed yet.

Focused failures, corrected cases and the affected development regressions above
have run. Frozen source and receipt binding, independent specification review,
full regression, final retained-application Settings checks, independent quality
review and the final production-rule self-audit remain pending. Development checks
do not replace the commitment's Cairn evidence. The full lifecycle and package
commitment remains in progress.

## Corrected specification review and quality findings

The corrected F1 snapshot `lifecycle-config-change-source-freeze-20260913T050617599937Z`
passed independent specification review. Its manifest SHA-256 is
`ef8d36c76d10325033b1a467136c121e84ea63a331afbbcb183bb74ea347ba29`, and archive
SHA-256 is `cc39ff942f89cb1324af2e8191ba32a9ab3f9f6629a387fdb6323e7e5e1646ce`.
The report is retained in scratch as `config-change-independent-spec-review-corrected.md`.

Independent quality review of the same frozen candidate returned **Not approved**.
The report `config-change-independent-quality-review.md` identifies two Important
findings. Q1: final Settings publication does not check Runtime.failed under the
Runtime mutex, so an unrelated persistence failure can hold ordinary execution
while still allowing private Settings replacement. Q2: EventSink assigns the exact
operation token after the Runtime exposes the new operation and releases its lock.
A concurrent phase finish or shutdown can see zero ownership and incorrectly
require recovery or fail cleanup. These are source-established defects; their
deterministic failing controls and corrections are assigned as separate follow-up
implementation work. No code was changed during either independent review.

The full regression command `cargo test --locked --all-targets --no-fail-fast`
exited 101 during compilation, before test execution, at
`lifecycle-config-change-all-targets-final-20260913T051020964189Z`. Rustc 1.95.0
reported an internal error, `uninterned StableCrateId`, while processing saved
incremental metadata for `plugin_model_runners`. The underlying cause is unknown.
This is not a full regression pass or a demonstrated application failure.

Before rebuilding, the exact matching incremental directory, test artifacts,
fingerprint and current application rlib were archived and hash-verified: 572
files, 776,350,031 input bytes. The bundle
`/home/shawn/demoncoder-check-tmp/config-change-rustc-ice-20260913T051632820662Z`
also retains the raw command/log and compiler/driver hashes. Archive SHA-256 is
`a9c10a3cf1f431509a22c27a4724eb340e7ba46c273aee4043d862fb93d010fb`.
After confirming no repository Cargo/rustc process remained, only the matching
incremental directory was moved intact into that bundle. Global configuration
and pinned backend binaries were unchanged. The unchanged source then passed
`cargo test --locked --test plugin_model_runners --no-run` at
`lifecycle-config-change-compiler-isolation-model-runners-20260913T051720312186Z`.
The 2,827-path freeze comparator still passed afterward at
`lifecycle-config-change-source-freeze-verify-after-ice-isolation-20260913T051750275616Z`.
That compilation result isolates a successful rebuild; it does not establish the
cause of the original compiler error. The source freeze is now released solely
for Q1/Q2 correction and this recorded review update.

## Q1/Q2 executed controls and correction

Q1 has an executed behavioral failure at `lifecycle-config-change-q1-red-20260913T052615314111Z`: an unrelated begin_phase persistence write failed after the gate passed, but the save returned Applied. The same exact test passes at `lifecycle-config-change-q1-green-20260913T052646294582Z` after final publication checks Runtime.failed under its existing mutex.

Q2 has three executed failures at `lifecycle-config-change-q2-red-behavior-20260913T053020282203Z`: concurrent shutdown rejected ownership, duplicate registration left an orphan operation, and failed registration retained token zero. The task-finish consequence is independently visible at `lifecycle-config-change-q2-finish-phase-red-20260913T053206936903Z`, where the paused post-insert boundary caused spurious recovery. That control temporarily restored the original Q2 behavior, then immediately reapplied the correction; the separate Q1 fix stayed in place. All three corrected ownership cases pass at `lifecycle-config-change-q2-green-final-20260913T053238934089Z`. The parent read these raw failed and passing results. Earlier test-support compilation failure is not counted as a behavioral control.

The correction binds the exact operation token inside the Runtime transaction before exposing the operation, and removes EventSink’s later assignment. Final focused compatibility checks, a corrected snapshot, independent re-reviews and full verification remain required. These controls do not change the selected commitment or claim its completion.

The corrected focused suites have now passed. Raw commands and results are in
`config-change-quality-corrections-handoff.md` in scratch. Settings Store passes
48 cases with one explicit installed-backend case ignored
(`lifecycle-config-change-corrections-settings-store-20260913T053319017243Z`);
Panel passes 12 (`corrections-settings-panel-20260913T053326553614Z`). The affected
non-tool, one-shot, tool-operation and ordinary runtime groups pass 10, 25, 25
and five cases respectively. Library/tests Clippy with warnings denied passes
at `lifecycle-config-change-corrections-clippy-20260913T053413357956Z`.
Formatting passes at `corrections-fmt-check-final-20260913T053453053234Z` after
the initial formatting check required two whitespace changes. Fresh final Q1
and Q2 runs pass one and three cases at `corrections-q1-final-20260913T053629100090Z`
and `corrections-q2-final-20260913T053641614923Z`. Shortened prefixes in this
paragraph all begin with `lifecycle-config-change-`. The parent inspected the
raw command metadata and test summaries. These results include the existing
post-publication failure, unrelated held-task, and abandoned-operation controls.

The worker released all source/test and Cargo ownership. Parent inspection found
only the expected four source/test paths changed since the prior review, plus
this parent-owned review record. Full regression, current installed cases,
retained-application Settings checks and independent re-review remain pending.
Ripwire's aggregate quality/test gates remain nonpasses; their concrete review
and executed behavioral evidence must remain visible in the final disposition.

## Q1/Q2 approved correction and full-regression findings

Independent SPEC and focused QUALITY re-reviews pass Q1/Q2 on snapshot
`lifecycle-config-change-source-freeze-20260913T053953572346Z` (manifest SHA-256
`6e53d1c51bea10604615b20ead51956689682920aa134197aaf983b4d438f267`, archive
SHA-256 `569b9846f0ef381e65bea68f55a039778443caa2278737018738973e45e216af`).
The reports `config-change-independent-spec-review-q1-q2.md` and
`config-change-independent-quality-review-q1-q2.md` retain their actual source,
control and static-delta examination. The parent read both reports in full.
Overall production approval remains withheld because of the following regression
failures, rather than any remaining Q1/Q2 correction request.

The complete `cargo test --locked --all-targets --no-fail-fast` command at
`lifecycle-config-change-all-targets-quality-corrected-final-20260913T054420653762Z`
compiled successfully and exited 101: 48 suites, 1,079 passed, four failed,
22 ignored. The final-check sequencer stopped there; later Clippy, installed,
build/retained-app and terminal qualification commands did not run. The exact
2,827-path comparator still passed afterward at
`lifecycle-config-change-source-freeze-after-failed-full-20260913T055540414928Z`.

One failure is a production policy-selection regression. The native workflow
resume test retained one correct Resume/Shutdown lifetime, but its observer's
prepare/run count was zero instead of two. `WorkflowSession::open_lifetime`
replaced the native session's own opening method with a connection-plan view.
The test's actual ToolExecutor holds its SessionStart plan, while the separately
deserialized connection has no plans. The wrapper freezes an empty generation,
so the inner native dispatcher refuses its real plan before runner admission.
The correction must preserve native delegation to freeze the actual executor
plans, while retaining host lifetime creation for external sessions. It must
preserve the original grant and ConfigChange ownership on all four connections.

The other three failures expose obsolete test classification after introducing
typed causal hook receipts. `external_transports_keep_snapshot_authority_and_charge_backend_invocations`
still expects ordinary Backend for a hook backend request. The matching and
unmatched compaction-funding helpers count only ordinary Model while expecting
the pre-compaction hook request in the same count. Their real request, summary,
task and checkpoint assertions passed before that mismatch. Correct these tests
to assert exact HookBackend/HookModel parent ownership and funding, and separate
ordinary summary/Creator requests from hook requests. Do not reduce expected
traffic or accept arbitrary old/new receipt variants.

Before any rebuild, the parent retained both exact failed test executables with
copy/source/copy hashes and source-stat checks. The library executable
`config-change-native-resume-failure-test-executable-20260913T054628764103Z` has
SHA-256 `35482dce5afafc601a49cc4337a3a04c46ee00fb055bf9eca536596cbaa2a51e`;
the integration executable `config-change-model-attribution-failure-test-executable-20260913T055411699514Z`
has SHA-256 `f01097171cd32a7713a92fc1466ed281972633d528245d61958ef4413313f234`.
Both live in scratch with JSON sidecars. Direct replay of the retained library
executable reproduces the same zero-versus-two failure without Cargo or a rebuild
at `lifecycle-config-change-native-resume-retained-exact-replay-20260913T055540394559Z`.
These are assertion failures, not compiler or native process crashes.

The reviewer and worker made no source changes while the full candidate was
frozen. The worker prepared a bounded correction separately; the source freeze
is now released for those recorded findings and this parent-owned review update.

## Native policy and typed-receipt corrections

The enhanced native control fails before correction at
`lifecycle-config-change-full-regression-native-policy-red-20260913T055726865702Z`:
the lifetime's frozen plans are empty instead of containing the exact registered
SessionStart digest. The corrected test passes at
`lifecycle-config-change-full-regression-native-policy-green-20260913T055817203694Z`.
It also requires the actual Resume lifecycle receipt to reference the retained
native lifetime, no unavailable-startup diagnostic, and both prepare/run calls.
`WorkflowSession::open_lifetime` now delegates to the native inner session and
keeps the connection-plan host lifetime path for external adapters. Settings
control activation remains after either path, using the original lifetime.

The three typed-receipt failures are corrected with exact parent and budget
assertions. The external hook backend is tied to the admitted tool operation;
ordinary summary/Creator model requests are counted separately from the allowed
PreCompact hook request. That hook must reference its actual lifecycle operation
and original SessionHooks funding. Related timeout, cancellation, missing-grant,
stopped-task and automatic-correction checks now include the typed hook variants,
so they cannot silently stop checking those requests. Existing traffic, outcome,
counter, deadline, usage and checkpoint expectations remain in place.

The final `plugin_model_runners` run after this related assertion audit passes
all 38 cases at
`lifecycle-config-change-full-regression-plugin-model-runners-final-20260913T060604994024Z`.
The native lifetime module passes three cases at
`lifecycle-config-change-full-regression-native-session-lifetime-suite-20260913T060032431523Z`;
Settings Store passes 48 with one explicit installed case ignored at
`lifecycle-config-change-full-regression-settings-store-suite-20260913T060224291871Z`.
Final Clippy with warnings denied, formatting, and diff checks pass. Their exact
raw commands and results, along with the four changed source/test hashes, are
retained in `config-change-full-regression-corrections-handoff.md` in scratch.
The parent read that handoff, the raw command/result summaries, and the complete
four-file correction diff against the prior reviewed archive.

Current raw Ripwire quality-delta remains exit 2 with 141 aggregate gating
findings at
`lifecycle-config-change-full-regression-ripwire-quality-final-20260913T060747931517Z`.
Test-gate remains exit 4, reporting 33 changed paths, 1,456 impacted symbols,
78 mapped test paths and 605 untested symbols at
`lifecycle-config-change-full-regression-ripwire-test-final-20260913T060749282424Z`.
Successful focused edit checks report no incompatible callers. Two earlier
workflow selector attempts failed and were corrected to the required canonical
path; they are not successful checks. These static nonpasses require an explicit
quality disposition alongside the new full regression and retained-app checks.

The worker released source and Cargo ownership. The corrected candidate is ready
for a new snapshot and independent re-reviews; full qualification has not yet
passed. No requirement or plan checkbox is closed by this repair alone.

## Full Rust pass and remaining qualification findings

Independent SPEC and QUALITY approve the native-policy and typed-receipt source
correction on `lifecycle-config-change-source-freeze-20260913T061244901530Z`.
The manifest SHA-256 is
`bb7de39f9194100a958f2c0e26eba33a668058c625042f2f3ffac500c10580c7`;
the archive SHA-256 is
`8bd66e1207e913835ffa3f34a3fc4eb521011e8fe56d24cf7d22ff3c7851389b`.
The parent read both complete `config-change-independent-*-review-native-attribution.md`
reports. QUALITY examined all 19 additional static rows, including 16 gating
findings, as test assertion growth and churn; no further source change was
requested. Both aggregate static checks remain nonpasses with their stated limits.

The full locked all-target Rust run passed **1,083 tests, zero failures and
22 ignored across 48 suites**, at
`lifecycle-config-change-all-targets-quality-corrected-final-20260913T061917938324Z`.
All-target Clippy, formatting and diff checks passed at suffixes
`clippy-all-targets-quality-corrected-final-20260913T063014245423Z`,
`fmt-quality-corrected-final-20260913T063014579464Z`, and
`diff-quality-corrected-final-20260913T063015397216Z`. These and subsequent raw
prefixes live under `/home/shawn/demoncoder-check-tmp/` and begin
`lifecycle-config-change-` unless shown in full. Earlier failed runs remain failures.

The final-check sequencer then stopped on the explicit installed owner case:
`installed-owners-quality-corrected-final-20260913T063015929449Z` failed one test
after 19.67 seconds. A denied save returned an error that did not contain the
expected synthetic gate reason. The assertion did not print the actual error,
so its cause is not yet established. Both installed executable hashes matched
the recorded pins. Before any rebuild, the exact failed library executable was
retained as `config-change-installed-owner-failure-test-executable-20260913T063120649049Z`,
SHA-256 `e41ad93324146ca8382978a229c3609233dd83615f4184f983b40a5cc4badadb`.
Its direct replay, `installed-owner-retained-exact-replay-20260913T063215172973Z`,
failed at an earlier boundary: the fixture saw no held provider request within
its existing 20-second bound. This is not reproduction of the first error's
reason. Both failures are assertions, not compiler or native process crashes.

Independent installed transport checks on the same frozen candidate passed:
`installed-batch-peer-after-owner-failure-20260913T063258277055Z` exercised both
actual external shared wrappers and three settled members; and
`installed-claude-batch-callback-after-owner-failure-20260913T063335313314Z`
observed one actual SDK callback after two equal members settled. These bound
general transport health without establishing the failed Settings-owner scenario.

The same candidate application built and was retained with SHA-256
`3dda56d7670ca5090ef1ddf37c21ef046f0ce57549f57139997ae543e8d1a720` at
`lifecycle-config-change-source-freeze-20260913T061244901530Z-demoncoder`.
The provider Settings suite passed 13 cases at
`provider-agent-settings-retained-after-owner-failure-20260913T063409579742Z`;
live Settings passed six at
`live-settings-retained-after-owner-failure-20260913T063528668818Z`.
The role suite passed five but its recovery case errored at
`role-settings-retained-after-owner-failure-20260913T063434667149Z` because
`App.record()` indexed an event log before its first session-record event arrived.

The parent traced the fixture: startup waits for the visible Prompt, then the
recovery test reads the independently written event log immediately. A scratch
diagnostic on the exact retained executable preserved and re-raised that original
IndexError. It observed an empty event list and a live application, then the actual
session-record event 28.98 milliseconds later with the process still live.
Raw prefix: `role-recovery-retained-observation-20260913T063627359441Z`.
A separate scratch control using the existing bounded event wait before the
original record read passed the complete unmodified recovery assertions in
2.336 seconds at `role-recovery-retained-wait-control-20260913T063722337865Z`.
The source correction should add that explicit readiness wait at the resumed
test boundary, preserving identity, allowance, no-replay and reviewer assertions.

The exact 2,827-path comparator passed after the independent suites. Importing
the role fixture in the scratch diagnostic created one new untracked Python
bytecode file; a later comparator correctly reported that added path. The parent
removed only that verified generated file and the exact comparator passed again
at `freeze-after-diagnostic-bytecode-cleanup-20260913T063850000883Z`, with no
changed source bytes or external inputs. The diagnostic scripts now disable new
bytecode generation. The failed comparator remains retained.

The source freeze is released for this recorded review update, the proven role
fixture readiness correction, and narrow safe installed-owner diagnostics. Those
diagnostics may report adapter, stage, error, invocation/request counts and marker
presence or byte lengths. They must not dump private configuration, environment
or workflow records. No installed-owner production fix, deadline increase, retry
or weakened denial assertion is justified without its actual cause.

## Installed-owner snapshot diagnosis and fixture correction

The instrumented installed run failed at
`lifecycle-config-change-installed-owner-instrumented-20260913T064152962602Z`.
Its separate retained `.full.log` contains the uncompressed RTK output; the
parent read that output and command metadata. Claude completed the scenario.
Codex reached the held provider request, ran ToggleGate once, and preserved the
original disk and role values, but reported `required gate snapshot is unavailable;
inspect workspace bounds, access and read-set declarations`. This localizes the
failure beyond initial transport and host admission. The generic message alone
does not establish the underlying capture error.

Source tracing found a fixture conflict with the required full-workspace read
set. The held-owner helper uses the same temporary directory for the admitted
workspace and installed-peer artifacts. The Codex launcher writes its live
wire/stderr files, PID and private home directories in its current directory.
The gate's default read set captures every admitted workspace entry and its two
scans must agree. Those ongoing fixture writes can make the final capture
unavailable after the gate runs. The source-supported correction is to separate
the peer artifacts from the admitted workspace, preserving the complete read set.
The corrected installed run remains necessary to verify this diagnosis.

The test-only correction may extend the two existing fixture launchers with an
optional `DEMONCODER_TEST_BACKEND_ROOT`; unchanged callers retain their existing
current-directory behavior. A generated relay sets that fixture root without
changing directory, so the installed child keeps its actual admitted workspace
while private homes and recording files live in the peer directory. Simply
changing directory before launching the backend would alter the test's workspace
semantics and is not an acceptable isolation fix. Actual installed shared-peer
cases must be refreshed because both launchers are shared. No production gate,
snapshot exclusion, grant, retry or timeout change is authorized by this diagnosis.

The first isolated-peer run passes both installed owners at
`installed-owner-isolated-peer-green-20260913T065024029350Z` (one test,
19.52 seconds). Both actual backend PID working directories match the admitted
workspace. The role readiness correction passes the six-case suite at
`role-settings-session-record-wait-20260913T064820879745Z` (11.002 seconds).
Shared installed batch/callback baselines also pass after the launcher changes.

The parent then compared the complete five-file fixture diff with the 061244
archive. That review caught a diagnostic change from the original top-level
`error.to_string()` denial predicate to searching the whole error chain. An
unrelated outer error could then pass merely because its context mentioned the
denial. Restore the original predicate and use the full chain only in the failure
message; run the installed case once more with that stricter assertion before
approval. No gate or snapshot behavior needs to change for this finding.

The first affected owner-matrix run also caught the new backend PID assertion
being applied to direct API sessions, which have no external backend process.
That failed run remains at `owner-matrix-fixture-corrections-20260913T065226643773Z`.
The assertion now applies exactly to Claude and Codex and still requires their
PID and actual working directory; a missing installed PID cannot be skipped.
The corrected native matrix passes 16 cases with the explicit installed case
ignored at `owner-matrix-fixture-corrections-green-20260913T065243619883Z`.

## Final fixture verification ready for review

The original top-level denial predicate is restored. The final exact pinned
installed-owner run passes one test, with both actual adapters and their live
workspace assertions, in 19.83 seconds at
`installed-owner-final-strict-predicate-20260913T070016421448Z`.
The parent read its actual output and standard-wrapper command metadata.
Post-correction formatting and diff checks pass at
`installed-owner-strict-predicate-fmt-final-20260913T070102046581Z` and
`installed-owner-strict-predicate-diff-final-20260913T070103050821Z`.
The full chain is now diagnostic text only.

The parent read the complete final handoff,
`config-change-installed-owner-role-fixture-handoff.md`, and the five-file diff
against the full-pass 061244 archive. Its final source copies and hashes are in
`config-change-installed-owner-role-fixture-final-strict-inputs-20260913T070121331990Z/`.
The shared launcher caller audit covers installed backend lifetime, Claude/Codex
installed suites and external compaction. The corrected owner test exercises
the optional artifact-root path; the refreshed shared batch cases exercise the
unchanged default path. Those actual default-path passes are
`launcher-public-batch-installed-20260913T065138399393Z` and
`launcher-claude-batch-callback-20260913T065155472027Z`.
The affected owner matrix passes 16 with one explicit installed test ignored;
the installed case passes separately. Native lifetime passes three, role Settings
passes six, Clippy with warnings denied passes, and all three edited Python
files parse. The handoff retains exact raw commands, earlier failures and their
corrections. Failed selector spellings and preliminary formatting are not passes.

Fresh static reports after restoration of the strict predicate remain nonpasses:
`fixture-strict-final-ripwire-quality-20260913T070301769051Z` exits 2 with 402
rows and 136 gating findings; `fixture-strict-final-ripwire-tests-20260913T070303014924Z`
exits 4 with 77 mapped test paths and 605 untested impacted symbols. Their changed
counts do not establish better coverage or replace a disposition of the fixture
delta. No baseline, acknowledgment or suppression was changed.

Only five test/fixture files and this parent review differ from the full-pass
snapshot. The Rust changes are in test-only modules; the launchers and role
script are fixtures. Application source is unchanged, so the retained executable
and its provider/live Settings passes still describe the delivered application.
The 1,083-test run remains bound to its original snapshot, with the corrected
fixture evidence declared separately. Repeating that full run for diagnostic
and fixture-only edits would not add production coverage. The next independent
reviews must examine this exact delta and its evidence before this prerequisite
is committed. The full lifecycle/package commitment remains unfinished.

## Final review and evidence binding

Both final independent agent reviews approve the bounded prerequisite. They
inspected the final five-file fixture delta, actual assertions and captured
commands, including the strict installed denial predicate and live backend CWD.
No source findings remain for this prerequisite. Earlier findings and failures
above remain part of the record.

All following artifacts are under `/home/shawn/demoncoder-check-tmp/`:

| Artifact | SHA-256 |
| --- | --- |
| `lifecycle-config-change-final-evidence-20260913T071612627233Z.json` | `f4a4ef8dc88bd920b57885efda5cf3195c9082b6fe10f687ef0d61fce5a9f0de` |
| `lifecycle-config-change-final-spec-review-20260913T071612627233Z.md` | `13764fe2bdb912a0fa7763d675daaad6cb52b3bd907b03bccafdcc207e2fe472` |
| `lifecycle-config-change-final-quality-review-20260913T071612627233Z.md` | `a2b2d0719f87cab77267e4a2e4ec17686e1e9e52598bf0adaf18126e5cbc9193` |
| `lifecycle-config-change-source-freeze-20260913T070353107217Z.json` | `862a9033ba467eca9efb640cae201d351d5ae7c6687605da36702cceb22c69ba` |
| `lifecycle-config-change-source-freeze-20260913T070353107217Z.inputs.tar.gz` | `871f600afc24ea2815093959159d79346b4f65f6ed4f03214cd10b74ecd5df5c` |

The evidence inventory binds 805 inputs, source archives, raw results, immutable
review snapshots, the retained application and failed test executables. It is an
integrity inventory, not a new test execution or a Cairn acceptance receipt.
The final source archive contains 2,827 tracked and nonignored paths with their
types and modes. The after-review audit matched every path and external input.
Only this final summary and the implementation plan are edited afterward;
their separate precommit audit must preserve all other source bytes and modes.

The full Rust run on the `061244901530Z` snapshot passed 1,083 tests across 48
suites, with zero failures and 22 ignored tests. All-target Clippy with warnings
denied passed. Production bytes are unchanged in the final fixture snapshot.
Later checks passed the strict installed Claude/Codex owner case, two shared
launcher cases, 16 owner cases and three native lifetime cases. The retained
application passed 25 provider, live and role Settings terminal tests. Focused
Clippy, formatting, Python parsing and diff checks passed. No new full Rust run
on the final fixture bytes is claimed; the exact raw commands are bound above.

The final quality report retains both static nonpasses: 136 gating quality
findings and 605 impacted symbols without mapped tests. Its comparison identifies
five removed helper-similarity findings, one three-line fixture verbosity finding,
and two enlarged fixture helpers. These changes support diagnostics, artifact
isolation and readiness. They do not prove additional coverage. The broader
previous triage still applies; no baseline, acknowledgment or suppression changed.

Controlled peers and installed backends do not establish live-provider behavior.
HTTP cancellation does not prove remote effects stopped. The generic exhausted
grant diagnostic, synthetic pending source delivery and common-path EOF/error
coverage retain their stated limits. Compiler and earlier process failures keep
their diagnostic records and unresolved causes. Neither those records nor the
22 ignored tests are recast as passes. Workspace/model transitions, the remaining
lifecycle matrix, public activation, packages and full conformance remain open.

## Parent production-rules self-audit

| Rule | Review conclusion for this prerequisite |
| --- | --- |
| 1. Understand before editing | The recorded decision and source reviews trace actual Settings publication, owner authority and failure boundaries. |
| 2. Smallest coherent change | Existing settings storage, runtime receipts, allowances and runners are extended; final repairs are fixture-only. |
| 3. Maintainability | Ownership and publication stages are explicit. Reviewed helper complexity retains the state transitions needed to explain cancellation and uncertainty. |
| 4. Boundary contracts | Old policy judges the exact proposal; current identity, original host lifetime, disk revision and source applicability are checked. |
| 5. Errors and secrets | Denial reasons remain attributed; proposal frames expose permitted structure and opaque bindings. Canary and private-source tests exercise the boundary. |
| 6. Security | Original authority, confined runners, exact causal operations and private settings protections remain enforced. |
| 7. Survivable state | Behavioral controls cover stale drafts, concurrent edits, failed receipts, cancellation and truthful applied/uncertain outcomes after rename. |
| 8. Reliability | Inspection stays outside publication locks; owned cleanup is bounded. Final fixture waits fail on exit or timeout without replaying work. |
| 9. Todo | Complete lifecycle dispatch remains the sole item in progress; its compound transition task stays unchecked. |
| 10. Verification | Full and affected checks, violating controls, installed-owner cases and terminal suites ran; source/evidence bindings distinguish their candidates. |
| 11. Honest reporting | Historical failures, ignores, static nonpasses and coverage limits remain explicit. This is no whole-commitment completion claim. |
| 12. Partnership | The authorized commitment and recorded judged decision set the scope; no unrelated feature or global configuration change is included. |
| 13. Release gate | Independent SPEC and QUALITY findings are resolved for this prerequisite. I am satisfied with this bounded implementation and its stated evidence limits. |
| 14. Plain writing | The current summary names actual save behavior, authority, outcomes and remaining work. Historical detail is retained for traceability. |
