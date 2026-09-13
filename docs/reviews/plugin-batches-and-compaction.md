# Explicit tool batches and context compaction

Status: Bounded implementation, specification review and quality review complete,
including the streaming and extension fixture repairs. The full regression and
post-run source audit passed. The complete plugin commitment remains open.

Current candidate: freeze `20260912T231807115940Z`, manifest SHA-256
`f70efd5cdc995df08d2111188813588087180a7e36d970f4c1b8f316f460663e`.
All four native compaction terminal cases, three output-limit terminal cases,
six installed/local-peer cases and affected fixture regressions passed.
All-target Clippy and formatting passed. The final full locked regression passed
1,025 tests with zero failures and 21 ignored across all 48 suites. The chronological
record retains each earlier failure, repair and proof limit.

The [decision](../decisions/own-explicit-tool-batches-and-context-compaction-through-existing-runtime-operations.md)
adds a shared explicit batch operation and owned native context compaction, and
connects actual external compaction callbacks to the managed lifecycle dispatcher.
It preserves individual tool evidence, current policy, original accounting owners
and the distinction between host operations and backend observations.

Design review corrected two restrictions before implementation: identical batch
member payloads are valid separate indexed operations, and applicable imported
Claude batch declarations must run on real host batches across connections with
their own dialect's outcome rules. An actual Claude SDK batch remains separately
attributed; no implicit Codex batch is invented. Manual compaction must preserve
the existing Codex `thread/compact/start` and Claude SDK command paths without
restarting task work or consuming deferred Creator context.

## Initial production failure demonstration

The parent ran the real terminal against controlled OpenAI and Anthropic API
peers using the retained baseline executable, SHA-256
`ab0623ded2530b53610712060a0d227dd6eb1778161c036e4b253392371864d8`.
Its source implementation is `6b36fadaf297c4aa5bf9cdab27c224e0071454b8`, included
in the merged baseline `38bb84bf714fcf8916c331a5cefcf7576a555686`.

Both cases received six ordinary responses. Entering `/compact` then caused a
seventh ordinary model request containing `{"role":"user","content":"/compact"}`
and all four ordinary tools. Both tests failed the intended assertion that manual
compaction must use a distinct tool-free request. These are behavioral failures,
not compilation or fixture setup errors. Both application instances closed through
the existing terminal fixture cleanup.

Raw command, output and exit status are retained under
`/home/shawn/demoncoder-check-tmp/lifecycle-batches-compaction-parent-pty-red-20260912T193931044695Z`.
The controlled request bodies, terminal captures, frozen test script and independent
parent audit are in
`/home/shawn/demoncoder-check-tmp/lifecycle-batches-compaction-parent-pty-red-artifacts-20260912T193930804258Z/`.
The frozen script SHA-256 is
`54ca98b158e1cde1f217b262bb855b2c2f0e0b9a6d0a76bf7d50d0a79697a0a0`.
Neither initial failure reached the completion-notice predicate. The fixture
initially waited for response text rather than the completed-turn status; the
corrected baseline demonstration below removes that timing ambiguity.

## Interim native terminal proof

The first run against the interim implementation correctly refused `/compact`
while a turn was still finishing. Both tests failed in fixture timing, with no
summary request. The output remains under
`lifecycle-batches-compaction-parent-pty-interim-20260912T200818729031Z`.
The fixture now waits for both the response sentinel and the ordinary
`DemonCoder · complete ·` status before sending another turn or session control.

With the same copied executable, both corrected terminal cases passed in
16.363 seconds. Executable SHA-256:
`be41a639298bcd4973242cc691aebb744217dd6a73dd5e6842abefe2050330b9`.
Each captured six ordinary requests, one tool-free summary request to the same
configured model, and one subsequent request only after an explicit new prompt.
The OpenAI request context shrank from 288,537 to 58,000 bytes; Anthropic shrank
from 288,197 to 57,932 bytes. Both retained the summary, current developer prompt
and new prompt. These byte counts use the same sorted JSON encoding on captured
provider context before and after compaction.

Output and exact command are under
`lifecycle-batches-compaction-parent-pty-idle-20260912T201020210837Z`.
Raw requests, terminal output, the frozen fixture and independent parent audit
are under `lifecycle-batches-compaction-parent-pty-idle-artifacts-20260912T201019958888Z/`.
The fixture SHA-256 is
`a8778e70d2de9c369e131094a8a8e299e2cac3d92c4a98889fa36974d7cefeed`.
All these paths are below `/home/shawn/demoncoder-check-tmp/`.

That exact corrected fixture also failed against the retained baseline in both
cases, in 16.467 seconds, at the intended tool-free-summary assertion. Each
seventh request still contained `/compact` and all four ordinary tools. The
raw request hashes match the original failure demonstration. Output is under
`lifecycle-batches-compaction-parent-pty-idle-red-20260912T202120576951Z`;
its frozen fixture and independent audit are under
`lifecycle-batches-compaction-parent-pty-idle-red-artifacts-20260912T202116318086Z/`.

These are interim production-path checks using controlled peers. They do not
accept a final source freeze or establish live-provider evidence.

The parent also exercised automatic compaction through the terminal on the same
interim executable. Both cases passed in 43.428 seconds. Each submitted twelve
ordinary prompts and captured exactly one tool-free summary between the tenth
and eleventh ordinary requests. The next actual request retained the submitted
prompt and summary. OpenAI context shrank from 519,380 to 60,460 bytes;
Anthropic shrank from 518,768 to 60,392 bytes. Output is under
`lifecycle-batches-compaction-parent-pty-auto-interim-20260912T202706979925Z`;
the requests, frozen fixture and independent audit are under
`lifecycle-batches-compaction-parent-pty-auto-interim-artifacts-20260912T202706961463Z/`.

The exact automatic fixture failed against the retained baseline in both cases
at the intended missing-summary assertion, in 46.635 seconds. Each baseline
captured twelve ordinary requests and no summary. Output is under
`lifecycle-batches-compaction-parent-pty-auto-red-20260912T202814191780Z`;
its captured requests, frozen fixture and independent audit are under
`lifecycle-batches-compaction-parent-pty-auto-red-artifacts-20260912T202814173478Z/`.
The final frozen candidate still needs all four terminal cases.

## Focused implementation and ownership review

The worker reports four native compaction checks and two batch checks passing
with `--locked`, retained under
`lifecycle-batches-compaction-native-and-batch-focused-20260912T200933067329Z`.
The native cases cover a smaller durable checkpoint with current prompt and exact
ordinary Unallocated usage, unchanged context after oversized output, cancellation
without a retry, and pre-hook allow/deny with stale inspected inputs. Earlier
test-only compilation errors are retained separately from behavioral failures.

The [taskless hook ownership decision](../decisions/bind-taskless-compaction-hooks-to-the-original-explicit-session-grant.md)
keeps the summary's ordinary model usage separate from explicitly funded Prompt
and Agent hooks. Review required the original native-session service owner and
retained counters across repeated compactions, with exact per-occurrence admission.
An unfunded or exhausted task cannot fall back to session funding.

The parent inspected the raw focused output for the session-grant case:
`lifecycle-batches-compaction-compaction-grant-green-20260912T202542069028Z`.
Its one test passed, exercising Prompt and Agent hooks charged to the live session
grant while the ordinary summary stays separate. Another run,
`lifecycle-batches-compaction-compaction-ownership-mcp-20260912T203010876797Z`,
passed five library cases and one MCP integration case. These cover typed summary
and tool-pair validation, atomic checkpoint/evidence retention, persistence holds,
invalidated hook owners and repeated-compaction service reuse with retained limits.
The direct native caller after-rename fault demonstration remains pending.

Actual installed Claude 2.1.267 passed managed manual compaction allow and deny
in `lifecycle-batches-compaction-installed-claude-managed-20260912T202707506729Z`:
allow produced both managed callbacks; denial produced only PreCompact and no
summary request or compaction boundary. The fixture declares its stable public
policy file as the inspected input. An earlier generated-file snapshot failure
was retained; shared launchers and private-home behavior were unchanged.

The parent also identified two interim source gaps: ended compactions lacked an
explicit completed-operation refusal, and summary parsing allowed a malformed
message role. The worker corrected both; the focused ownership and summary cases
above passed. Fresh independent specification review still follows the final
focused candidate. Codex callback delivery, further source-batch correlation and
the final recovery controls remain in progress.

A later negative case failed in
`lifecycle-batches-compaction-exhausted-compaction-red-20260912T203714413558Z`.
With one funded hook model call, PreCompact spent that call and the separately
owned summary applied. PostCompact correctly refused another model request, but
optional-handler failure handling reported overall compaction success. This was
a continuation-status defect, not an unauthorized provider call. The correction
treats the native model continuation gate as held while preserving the applied
checkpoint and original usage. It passed in the seven library and three
model-runner cases under
`lifecycle-batches-compaction-native-recovery-task-20260912T204136460619Z`.
Those cases also cover the actual native caller after a checkpoint rename succeeds
but directory synchronization fails, the failed-runtime latch, retained child
checkpoint ownership, and stopped tasks retaining their original allocation epoch.
The first directory-sync fixture predicate did not fire and was corrected; that
ineffective injection remains in its original failed run.

The worker reports three installed integration tests passing in 58.20 seconds
under `lifecycle-batches-compaction-installed-managed-both-20260912T204213938952Z`.
They exercise Claude manual compaction allow/deny, Codex manual compaction
allow/deny, and an actual Claude SDK batch containing two equal write payloads
with distinct tool IDs and one batch observation. Both pinned executables remain
unchanged and use controlled model peers. Earlier incorrect Codex notification
expectations and the observed Claude absent-UUID continuation are retained in
the failed runs; exact continuation-owner negatives are still being verified.

The exact SDK continuation-owner case subsequently passed in
`lifecycle-batches-compaction-source-batch-identity-20260912T204704444744Z`.
The real asynchronous contribution ordering case passed in
`lifecycle-batches-compaction-deferred-context-20260912T204621037168Z`:
automatic summarization sees neither the contribution nor its reservation, and
the next Creator request receives it once without summary use of the hook grant.

The batch cancellation case passed in the earlier focused batch run, retaining
the completed first effect without observing an unsettled batch. That run's
model-result case used an invalid Observer classification for a Claude source
declaration. The corrected fixture uses the existing Combined classification;
it does not implement or relax the separate public conversion control. Its
corrected result will be recorded with the final focused candidate.

## Compiled violating controls

The parent inspected the raw output and mutation records for three controls.
Each modified production source, compiled with `--locked`, ran one matching
behavioral test and failed its intended assertion. Each mutation record retains
the exact patch, original and mutated source hashes, and successful restoration.

| Temporary violation | Observed failure | Raw output prefix |
|---|---|---|
| Remove member-settlement checks | A completed result crossed an unfinished observer boundary | `lifecycle-batches-compaction-negative-batch-20260912T205157780092Z` |
| Remove both strict size-reduction checks | Native compaction accepted a nonshrinking replacement | `lifecycle-batches-compaction-negative-shrink-20260912T205242688064Z` |
| Claim Creator context before automatic compaction | The summary received the contribution reserved for the next Creator request | `lifecycle-batches-compaction-negative-deferred-20260912T205331735499Z` |

The deferred-context control also produced a subsequent fixture timeout after
the intended assertion; that full output is retained. Independent review found
that the scratch capture helper named source snapshots by basename. The two
shrink-control files are both named `compaction.rs`, so the runtime snapshots
overwrote the native snapshots. Six of eight retained snapshot hashes match; the
two native shrink snapshots cannot be verified from those files. Full-path patches
and hashes remain in the receipt, whose full-path restoration checks passed,
and the raw behavioral failure remains intact. Historical artifacts are unchanged;
future captures must use unique full-path identities. None of these runs is a
passing check. Their corresponding `mutation-batch`, `mutation-shrink` and
`mutation-deferred` JSON records are below the same scratch prefix. Restored
focused verification and the final source freeze must follow before acceptance.

## Final framing finding

The parent identified one further defect before specification review: host batch
input used the original requested arguments after a member's PreToolUse handler
had rewritten them. The actual effect and original receipt remained intact, but
the batch observer could be told different arguments from those executed. The
worker confirmed it and corrected the frame to use the final admitted call.
A focused test compares the actual effect, framed input and original/final
receipt, and includes a denied edit and a shell command that executes and exits 7.
The worker reports those cases passing. A subsequent schema audit found that
Claude's closed item schema permits only `tool_name`, `tool_input`, `tool_use_id`
and `tool_response`. The proposed extra admission/effect fields must be removed
from the frame; those facts remain in authoritative ToolReceipts and the original
failure responses. The corrected test also validates the frozen schema. No
semantic profile or validator relaxation is authorized.

The genuine failure is retained under
`lifecycle-batches-compaction-rewritten-input-red-fixed-fixture-20260912T211159680497Z`:
filesystem assertions passed before the original-versus-final framing assertion
failed. The earlier attempted demonstration used an incorrect exact-name matcher
and never rewrote the member; it remains classified as a fixture failure.

The initial freeze `20260912T210741539583Z` predates this correction and is
superseded diagnostic evidence. No independent specification review or final
parent terminal acceptance used it. A new freeze must follow the correction.

## Corrected freeze and independent specification review

The corrected source freeze is `20260912T211855437649Z`, manifest SHA-256
`55672c4d134f26b3f3e5d957cb340c6c5771d467ea5a83382f46d7af8005429e`.
The parent independently verified all 371 semantic inputs, the exact 44 changed
source/test paths, retained artifact hashes and both unchanged installed binaries.
The retained application SHA-256 is
`df8c28c50268e227cde799c045d2028dc592b1e50595025e77d83f4194f88508`.

All four parent manual/automatic terminal cases passed in 62.355 seconds against
that executable, under
`lifecycle-batches-compaction-parent-pty-final-20260912T212103147222Z`.
The independent audit and raw requests are in
`lifecycle-batches-compaction-parent-pty-final-artifacts-20260912T212103126522Z/`.
Their request hashes and context sizes match the earlier interim cases; every
case made exactly one summary request and retained the required prompt pins.
All 371 semantic input hashes were unchanged after the run.

Fresh specification review identified one source blocker, S1 (initially F1): native compaction
model-hook funding is checked before trigger applicability. An auto-only hook
can therefore block manual taskless compaction without a grant despite not
matching the operation. The fix must filter inapplicable hooks before enforcing
their funding, while matching unfunded hooks still hold and spend nothing.
The completed review is FAIL with this one blocker. The implementer is repairing
it with a production-path failing control and matching funded/unfunded checks.
A new source freeze and review must follow before the full regression run.

The worker handoff explicitly identifies unexercised combined variants: interrupted
batch restart, cancellation after compaction application before deferred Creator
delivery followed by reopening, installed Codex PostCompact continuation holds,
and the full once/async and stale-child matrix. Existing checks must not be
represented as those exact end-to-end variants. The completed review records these as proof limits and targeted follow-ups,
without identifying another source defect. The full lifecycle commitment remains
open; these observations do not establish the missing end-to-end variants.

## Trigger applicability repair

The new production-path regression reproduced S1 in
`lifecycle-batches-compaction-s1-trigger-red-20260912T213550399778Z`:
the unmatched auto-only Prompt PreCompact handler blocked manual compaction.
The matching-handler control passed. After moving matching before the funding
hold, both tests passed in
`lifecycle-batches-compaction-s1-trigger-green-20260912T213704055884Z`.
Together they exercise 32 combinations across Prompt/Agent, Pre/PostCompact,
manual/auto triggers, matching and unmatched handlers, and missing or exhausted
funding. Matching PostCompact refusal keeps the already applied ordinary summary
and its Unallocated usage without another hook request. No case starts a task.
Formatting, final freeze and independent re-review follow; this focused pass alone
does not close the task.

## S1 review and broad regression findings

The same specification reviewer approved S1 on freeze
`20260912T214037170291Z`, manifest SHA-256
`81be74d09fe8e511c6fd5e6f4252b4f784038a981256f8a2c78de06d0761bb0c`.
The parent independently verified 580 declared source/evidence/helper identities
and all 44 changed source/test paths. The retained executable SHA-256 is
`d7a6a24b8dfcee73e6e76b701e136c6e5770e308d9eeb8db0e2ba529e20e1ea0`.
All four parent terminal cases passed in 61.607 seconds in
`lifecycle-batches-compaction-parent-pty-s1-20260912T214146779340Z`.
Their artifacts and independent audit are in
`lifecycle-batches-compaction-parent-pty-s1-artifacts-20260912T214146756827Z/`.
The actual request hashes match the preceding four-case run; all 371 semantic
inputs remained unchanged afterward. An initial parent launch failed before any
test with FileExistsError because the parent pre-created the fixture's exclusive
artifact directory. The corrected launch used a new absent path; source and
fixture were unchanged. That setup failure is retained at
`lifecycle-batches-compaction-parent-pty-s1-20260912T214129960102Z`.

After SPEC PASS, the parent started the full `cargo test --locked --all-targets`
run alongside fresh quality review. It stopped in the library suite: **419 passed,
1 failed**, with the remaining suites not reached. The retained run is
`lifecycle-batches-compaction-all-targets-s1-20260912T214426371184Z`.
The existing StopFailure matcher test caught a compatibility regression: the new
absent `trigger` field serialized as null and changed the legacy matcher bytes.
The existing assertion and legacy representation must remain; the new optional
field needs compatible serialization.

The parent also ran all-target Clippy with `--locked` and `-D warnings`. It failed
in `lifecycle-batches-compaction-s1-clippy-all-targets-20260912T214557906253Z`:
the Store fault setter is unused when the integration target directly includes
Store, and the new native persistence test uses `.err().expect()` instead of an
accepted error assertion. Earlier lib/bin-only Clippy did not check these targets.
Both failures must be fixed before delivery, without changing the persistence
contract or weakening lint checks. Source remains frozen while quality review
consolidates its findings; no full passing regression is claimed.

Final-source static runs are retained as
`lifecycle-batches-compaction-s1-static-quality-20260912T214505212497Z` (exit 2)
and `lifecycle-batches-compaction-s1-static-tests-20260912T214544514643Z` (exit 4).
They report 105 gating major findings and 46 test obligations with 556 unmapped
impacted symbols. They are review inputs, not passing or executed tests.

## Completed quality review and authorized repairs

Fresh quality review returned CHANGES REQUIRED with two Important findings:
Q1, legacy matcher serialization; Q2, the all-target lint failures above. It also
identified Q3, a small input-bound mismatch: native compaction caps the encoded
history at 1 MiB and then adds 275 bytes of summary instructions. The complete
summary input can therefore exceed its stated cap. The repair must check the
complete input before model admission, without weakening the decision or README.
A near-boundary failure must make no summary request or summary allowance debit;
a valid near-boundary control must remain usable.

The reviewer inspected the actual increased function complexity, bridge and
admission boundaries, replay/recovery paths, raw checks, and all 14 production
rules. It established no further runtime defect. The report is retained at
`/home/shawn/demoncoder-check-tmp/lifecycle-batches-compaction-quality-review.md`,
initial SHA-256 `0b404d442250d4b5978391130078422a527b5745d805b56ead5ab1915abcb840`.
Its disposition of static exits 2 and 4 does not turn them into passing tests.
The missing combined recovery/continuation variants remain explicit proof limits.

The implementer is authorized to repair all three findings together. Keep the
legacy matcher assertion; omit an absent trigger. Exercise the existing Store
fault setter through its existing durability test, including predicate-delayed,
independent-instance and one-shot controls, while retaining installed-payload and
reopen assertions. Use the accepted error assertion without changing production
types or suppressing the lint. Source/focused proof will be frozen again before
specification re-review, full locked regression and quality re-review.

Parent corrected the public README to describe idle `/compact`, bounded native
automatic compaction, preserved prompt/evidence/allocation and explicit ordered
`tool_batch` calls. The reviewer checked those statements against current source
and identified Q3 during that check. README is already in the declared Cairn
footprint and will be bound separately in final delivery evidence.

The Q3 production-path failure is retained in
`lifecycle-batches-compaction-q3-input-red-20260912T215850712191Z`.
Its one test compiled and failed the intended provider-request-count assertion.
Through the Anthropic adapter and a local HTTP fixture, it first measured actual
summary framing and accepted a complete input of exactly 1 MiB. A complete input
one byte over that bound then reached the provider when zero requests were
required. The fixture derives framing from the actual request instead of copying
the 275-byte implementation constant. This is a behavioral failure, not a setup
or compilation error. The corrected complete-input guard passed in
`lifecycle-batches-compaction-q3-input-green-20260912T215941830043Z`.
The exactly 1 MiB control applies through the actual Anthropic adapter with one
original Task model debit and input/output usage 11/7. The one-byte-over case
retains its context and creates no summary request, model operation or debit.
The remaining repairs and complete checks still follow.

The Q1 legacy serialization assertion passed unchanged in
`lifecycle-batches-compaction-q1-legacy-green-20260912T220113011580Z`.
The revised Q2 Store durability test passed in both its library and independently
compiled integration targets in
`lifecycle-batches-compaction-q2-store-green-20260912T220149035061Z`.
The combined focused run,
`lifecycle-batches-compaction-q123-focused-green-20260912T220217037908Z`,
passed nine library and six model integration tests. It includes present-trigger
serialization/identity, all 32 applicability cases, exact/over-limit summary input,
original owners, and actual native persistence recovery.

All-target Clippy with `--locked` and `-D warnings` passed in
`lifecycle-batches-compaction-q123-clippy-all-targets-20260912T220632183724Z`.
These close the observed focused failures; final source binding, independent
re-reviews and a passing complete regression are still required.

## Q123 final checks and shared fixture failure

The Q123 freeze is `20260912T220827669102Z`, manifest SHA-256
`68fadfd53aef8194bfbcb5b28fd9afcf126c60c6ce3dcb002a3935a3834f86c2`.
The parent independently checked 606 declared hashes, all 371 semantic inputs
and the 44 changed source/test paths. Specification re-review passed. Quality
re-review resolved Q1, Q2 and Q3 in source but withheld approval pending complete
regression and the final six installed-backend/local-peer cases.

All four final native terminal cases passed in 64.493 seconds against the retained
Q123 executable, SHA-256
`d90cfb1aba05eba6323d8f7343b4ecdcc3c8acf16c80494b302fb199efb14c86`.
The raw run is
`lifecycle-batches-compaction-parent-pty-q123-20260912T220922300093Z`.
The independent audit and captured requests are in
`lifecycle-batches-compaction-parent-pty-q123-artifacts-20260912T220922280522Z/`.
Request hashes, context sizes and prompt pins match the preceding four-case run.
The separate output-limit terminal suite passed all three cases in 4.166 seconds,
under `lifecycle-batches-compaction-parent-output-limits-q123-20260912T220922260438Z`.
All semantic input hashes still matched after these runs.

The full locked regression then exited 101 with **927 passed, 2 failed and 20
ignored across 36 completed suites**. Later suites were not reached. Its raw run
is `lifecycle-batches-compaction-all-targets-q123-20260912T221258883426Z`, log SHA-256
`7ce07d364efa228d494c8aaa21dbb53b9c21d796539cfb9b48a0e390699fff25`.
All 421 library cases and all 48 post-tool cases passed. Two queue-runtime tests
timed out waiting for backend events: cancellation with a full advisory queue and
drained corrections counting toward the turn limit. This is not a full-suite pass.

Before rebuilding, the parent retained the exact failing queue executable at
`lifecycle-batches-compaction-queue-failure-binary-20260912T222610365475Z`, SHA-256
`5fc8b22e45164aa4b1a952e80c9f9200a87592afe75a6b062ac7677a4cfae396`.
A bounded rerun used that executable and the existing private fixture request
recorder. It reproduced the first timeout and captured five Codex startup messages,
ending in `thread/start` with exactly `read`, `write`, `edit`, `bash`, `tool_batch`.
No Claude iteration occurred. Replaying those messages into the unchanged shared
fixture retained its actual assertion failure: `tests/backend_fixture.py:150`
still required exactly four tools. Earlier startup replies succeeded.

The diagnostic result is
`lifecycle-batches-compaction-queue-diagnostic-20260912T222934391940Z/diagnostic-result.json`,
SHA-256 `4b6cb3501a0acd11dcac20f9dd9dda27737ff77d53bd0c2e9dc57d24c9b9a2d4`.
It binds twelve raw artifacts, the retained binary and unchanged Q123 inputs.
This establishes a stale fixture expectation; it does not demonstrate a product
queue defect or the earlier private-home mount failure. The authorized repair
adds `tool_batch` to the exact expected set, retaining equality and conditional
LSP validation. Affected checks, a new source binding and re-review must precede
the next full run. The original failed run and diagnostic remain unchanged.

Current static checks exited 2 and 4 respectively:
`lifecycle-batches-compaction-q123-static-quality-20260912T221015224236Z` reports
332 findings, including 107 gating findings, without suppressions;
`lifecycle-batches-compaction-q123-static-tests-20260912T221118385842Z` reports
46 mapped entries and 556 unmapped impacted symbols. The reviewer examined the
increased loop/admission complexity and cohesive typed helpers, the Store test
changes and the complete summary-input guard. These results inform review;
they do not establish executed test coverage or passing static gates.

## Exact tool catalog fixture repair

Adding `tool_batch` to the shared backend fixture's strict expected set passed
all seven queue-runtime tests, including both original failures across Codex and
Claude, in `lifecycle-batches-compaction-queue-fixture-green-20260912T223522564668Z`.
Six startup controls still rejected a missing batch tool, an unexpected tool,
missing enabled LSP and unexpected disabled LSP; the exact ordinary and enabled
LSP inventories passed. Existing actual Codex and Claude terminal tool cycles
also passed. An initial scratch caller wrapper failed before launch because it
omitted a required `wrong_edit` fixture field; that failed setup remains retained
separately from the corrected `caller-v2.py` run.

A bounded caller scan identified the same assumption in five other fixtures.
Actual native tool-cycle, outside-path access and LSP requests failed their old
tool-count assertions. Captured startup replay failed the strict subagent and
orchestration worker assertions. All five failures are retained under
`lifecycle-batches-compaction-shared-catalog-red-20260912T223903545437Z`.
The actual requests contained the five ordinary tools, plus LSP when enabled.

The repair changes exactly one inventory assertion in each of six Python files:
`backend_fixture.py`, `terminal_session.py`, `host_access.py`, `lsp_adapters.py`,
`subagent_backend_fixture.py` and `orchestration_backend_fixture.py`, all in `tests/`.
It preserves strict equality, LSP conditions and empty-tool Oracle/advisor checks.
It changes no application, Cargo, compatibility-profile or backend source.
The same five catalog cases passed in
`lifecycle-batches-compaction-shared-catalog-green-20260912T224044029933Z`.
Actual Codex child assignment, isolated effects and worker/advisor caller checks
passed in `lifecycle-batches-compaction-shared-child-callers-green-20260912T224151103865Z`.
All six fixtures parsed and `git diff --check` passed. No full-suite pass is
inferred from these focused results.

The final fixture candidate is `20260912T224335710042Z`, manifest SHA-256
`ca7576098213a15eb479f6391978d4ea7dd45b17bcd5e1acafa2317004770523`.
It binds 371 semantic inputs and 50 total changed source/test paths. The parent
independently verified 707 declared hashes, the base and exact changed-file
inventory without a mismatch. The application is a verified copy of the Q123
executable above; the six fixture changes required no application rebuild.
The same specification reviewer approved the exact fixture delta and its raw
failure/control evidence. Quality re-review and full verification remain pending.

The refreshed static quality run,
`lifecycle-batches-compaction-final-fixtures-static-quality-20260912T224430413749Z`,
exited 2 with 338 findings and 113 gating findings. The six added rows are fixture
churn; no preceding row changed. The refreshed test map,
`lifecycle-batches-compaction-final-fixtures-static-tests-20260912T224459653710Z`,
exited 4 with 97 mapped entries and 565 unmapped impacted symbols. Its larger
Python caller set follows the shared fixture edits. Neither result is a pass.

## Final installed and local-peer checks

`lifecycle-batches-compaction-final-all4-fixtures-20260912T224847272559Z` passed
all six cases in 53.44 seconds with `--locked --include-ignored --nocapture`.
The shared three-member batch ran through both native API adapters and actual
installed Claude/Codex. The actual Claude SDK equal-member batch retained one
settled batch callback. Managed manual compaction on both installed backends
retained two callbacks when allowed and only PreCompact when denied.

The exact unchanged backends are Claude 2.1.267, SHA-256
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`, and the
managed Codex build, SHA-256
`c4d77a245a7fcda26f606bb4f726b59ede5fcf0eb322fb7d625a759fc150592a`.
These cases use controlled local model peers. They establish current managed
delivery for the exercised paths, without claiming live-provider evidence or
the untested installed-Codex PostCompact-refusal continuation variant.

## Final broad regression: streaming-limit interaction

The locked full run
`lifecycle-batches-compaction-all-targets-final-fixtures-20260912T224947632694Z`
exited 101 with **930 passed, 2 failed and 21 ignored across 38 completed suites**.
Later suites were not reached. All seven queue-runtime tests and all 48 post-tool
tests passed. In `tests/reliability_streams.rs`,
`anthropic_accepts_exact_byte_bound_after_completion` and
`anthropic_bounds_inline_input_before_admitting_any_response_calls` failed at the
shared helper's success assertion with `Context compaction held: source exceeds
1 MiB`. The excess-before-completion test passed. The raw result alone does not
establish whether the defect is runtime timing or an outdated fixture assumption.
Neither streaming admission assertions nor compaction bounds may be weakened
merely to obtain a pass.

Raw log SHA-256:
`57d92e1bee2edd175535fc4f360ee67ce4b65d27a1207e9737d0803dc49bdbb4`.
The parent preserved the exact failing test executable before any rebuild at
`lifecycle-batches-compaction-stream-failure-binary-20260912T230146633389Z`,
SHA-256 `9cb4a208bad88be2aa0c19679121106ab06be0290b32596a60ec332c959476c2`.
Its original/copy/original hashes matched. The post-run parent audit
`lifecycle-batches-compaction-parent-audit-20260912T230239980384Z.json` verified
707 declared hashes, the base and exact changed-file inventory without a mismatch.
The retained executable reproduced the exact-size failure in
`lifecycle-batches-compaction-stream-retained-red-20260912T230435193560Z`.
A narrow temporary-workspace recorder observed the first write at 166 ms and the
full second write at 198 ms: 1,048,540 bytes representing 524,270 UTF-8 characters.
Both effects preceded the later preparation hold. Its diagnostic artifacts are
in `lifecycle-batches-compaction-stream-diagnostic-20260912T230429490718Z/`.

The source trace and recorded effects establish separate phases. Anthropic accepts
the exact 1 MiB arguments and native execution completes both calls. At the next
empty-pending boundary, their history plus framing exceeds the separate compaction
source cap. The recorded compaction decision requires that failed preparation
hold continuation. REL-003 requires exact argument accumulation and early oversized
response refusal, with no admitted oversized calls; it does not require this later
continuation to succeed. The fixture's unconditional whole-turn success assertion
conflated those phases.

These tests use a bare EventSink without a durable workflow owner. Ordinary model
calls support that arrangement, but compaction requires durable ownership. No
automatic-compaction policy exemption was established for bare callers, and no
scheduler bypass is authorized. The narrow repair is confined to the streaming
fixture: retain precompletion admission/effect checks and early oversize rejection;
require exact UTF-8 effects before accepting only the specifically attributed
compaction preparation hold, with one request and no false completion. Add a
below-threshold control that completes normally with paired tool results in its
second ordinary request. Bound server cleanup and record unexpected requests.
Runtime behavior, both limits and original failure evidence remain unchanged.

The revised streaming tests passed all four cases in 0.58 seconds under
`lifecycle-batches-compaction-streams-phase-v2-20260912T231251188713Z`.
They retain full-content UTF-8 equality, no admission before completion, specific
early parser refusal, the exact later preparation hold with one request, and a
256 KiB control that completes with paired results in its second ordinary request.
An initial test-only compile failure used `expect_err` on a result whose success
type lacks Debug. The fixture now uses an explicit pattern match; no production
type was changed. That compile failure is not a behavioral failure demonstration.

Before repeating the full run, the worker ran all ten targets skipped after the
streaming failure with `--no-fail-fast`. The retained run
`lifecycle-batches-compaction-streams-remaining-targets-20260912T231345451786Z`
ran every target and reported 91 passes and one failure. The extension test still
expected four built-in tool definitions and five with its delegate extension.
The new shared batch makes those counts five and six. The failure preceded its
execution, receipt and unknown-tool assertions. Its exact executable was retained
before further Cargo work at
`lifecycle-batches-compaction-tool-extensions-failure-binary-20260912T231422277067Z`,
SHA-256 `ae3a525338f2d0d606a605a6b548717af0af860d5ac1b3fe3ef334fb2b9333b7`.

Only those two expected counts changed in `tests/tool_extensions.rs`; all other
assertions remained. Both extension tests passed in
`lifecycle-batches-compaction-extension-counts-green-20260912T231558450091Z`.
All ten remaining targets then passed **92 tests, zero failed and zero ignored** in
`lifecycle-batches-compaction-streams-remaining-final-20260912T231605260552Z`.
Final formatting, all-target Clippy with `-D warnings`, and diff checks passed.
These focused runs do not convert the earlier failed full run into a pass.
The next full run will use `--locked --all-targets --no-fail-fast` to retain every
target outcome even if a failure occurs.

## Final streaming fixture candidate

The final freeze is `20260912T231807115940Z`, manifest SHA-256
`f70efd5cdc995df08d2111188813588087180a7e36d970f4c1b8f316f460663e`.
Only `tests/reliability_streams.rs` and `tests/tool_extensions.rs` differ from
the approved preceding fixture freeze. Production code, the application binary,
Cargo inputs, profile/schema, installed backends and README remain unchanged.
The parent independently verified 747 declared hashes, all 371 semantic inputs,
the base and the exact 52 changed source/test paths. The same specification
reviewer approved the two-file delta and its raw behavioral evidence.

Current all-target Clippy, formatting and diff checks passed under
`lifecycle-batches-compaction-streams-final-clippy-20260912T231640168105Z`,
`lifecycle-batches-compaction-streams-final-format-20260912T231640165864Z` and
`lifecycle-batches-compaction-streams-final-diff-20260912T231640200672Z`.
The refreshed static quality run,
`lifecycle-batches-compaction-final-streams-static-quality-20260912T231859211935Z`,
exited 2 with 345 findings and 118 gating findings. Its seven added rows describe
four streaming-test churn findings, fixture length, a minor complexity increase
and the new harness-called test. No earlier row changed. The refreshed test map,
`lifecycle-batches-compaction-final-streams-static-tests-20260912T231922513224Z`,
exited 4 with 95 mapped entries and 565 unmapped impacted symbols. These remain
review inputs, not passing checks or evidence that every mapped Python suite ran.
Quality review accepted the two test-file changes without a new source blocker.
It verified that the smaller map drops the two edited Rust test files despite
their actual execution; this is a mapping limit, not improved coverage. The
95 entries comprise 51 Python files and 44 Rust paths. The larger streaming
fixture retains explicit phase and cleanup assertions; its measured size and
complexity are recorded rather than hidden by a metrics-only rewrite. At this
stage, final approval awaited the full run and post-run source audit recorded below.

## Complete regression and final review

The final `cargo test --locked --all-targets --no-fail-fast` run passed **1,025
tests, with zero failures and 21 ignored, across all 48 suites**. Its raw prefix is
`lifecycle-batches-compaction-all-targets-final-streams-20260912T232126527689Z`.
Log SHA-256:
`0276a6fba5515dd135066940cbdc8c940f84ef7a00087fe8ef245cfec7b04c2e`.
Command/exit metadata SHA-256:
`4fee239254f6fce333cfb987a66dae52088e1bd49ca052a9f9262034f6c46b06`.
Its separate summary retains every actual target. No ignored test is counted as
passed; the six explicitly run installed/local-peer cases remain separately
recorded above.

The post-run parent audit
`lifecycle-batches-compaction-parent-audit-20260912T233258841762Z.json` verified
all 747 declared hashes, the base and the exact 52 changed source/test paths
without a mismatch. This is a passing full run on the recorded candidate, with
the earlier failed runs preserved. The quality reviewer independently rehashed
all 747 entries, reconciled the actual target list and approved the bounded task.
No unresolved source finding remains within that review. All 37 top-level Rust
targets in the static map appear in the full run; seven contain only ignored
cases. Neither their presence nor the 51 mapped Python paths establishes that
their behavior executed.

The immutable parent evidence manifest is
`/home/shawn/demoncoder-check-tmp/lifecycle-batches-compaction-final-evidence-20260912T233655034994Z.json`,
SHA-256 `018d5e8768032448cb8f6504c03954ec1ef0fa8ddc7ec0e4e8b412806c12a313`.
It binds the frozen source, raw regression and target summary, post-run audit,
immutable specification and quality review snapshots, README, terminal captures,
installed/local-peer checks, lint and static nonpasses. Mutable parent records
are excluded to avoid circular hashes. Review snapshots use the same timestamp:
`lifecycle-batches-compaction-final-spec-review-20260912T233655034994Z.md`,
SHA-256 `bf573b7f198ff590948f1df84768f996db1ae07e497625021823e012f18896cb`, and
`lifecycle-batches-compaction-final-quality-review-20260912T233655034994Z.md`,
SHA-256 `d2e1f14f89210f4671b09d0b8230e206abdb8e7ff7bf9166b952cac1269a36cf`.

The final native terminal and output-limit checks passed on unchanged application
source. They were not rerun for the last test-only repairs. The earlier 991 passing
Rust tests and 17 ignored cases describe the previous prerequisite. These bounded
checks do not establish live-provider qualification, complete the full lifecycle
matrix or replace Cairn evidence. The plan retains interrupted-batch reopen,
applied-compaction cancellation before deferred Creator delivery and reopen,
installed Codex PostCompact refusal followed by attempted continuation, and the
remaining once/async/stale-child combinations.

## Parent production-rule self-audit

The source and two independent reviews establish the requested behavior using
existing admission, ownership, persistence and runner paths. The repairs preserve
legacy contracts and strict bounds; specific failures and uncertain publication
remain visible. Bounded resource use, exact allowance ownership and original
effect evidence were challenged by the recorded behavioral controls. The full
regression, targeted production-path checks and lint passed against the frozen
inputs. Static findings, ignored tests and missing combined cases remain explicit.

All 14 production rules were reconsidered. No further revision is required for
this bounded task. Only its verified checkbox is closed; complete lifecycle
dispatch remains the sole item in progress. The shared decision and full
commitment remain unfinished.
