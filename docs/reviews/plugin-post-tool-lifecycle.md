# Synchronous tool lifecycle prerequisite

Status: Three quality repairs passed affected tests. A supplemental specification
review identified incomplete structured replacements; that repair and renewed
independent reviews are in progress.
This prerequisite does not discharge the full lifecycle commitment.

## Pinned Claude source observations

`python3 tests/plugin_post_source_inputs.py --claude /home/shawn/.local/share/claude/versions/2.1.267`
passed nine controlled source cases: success, failure, array/string replacements,
four falsy replacement values, and an invalid object replacement.
Each case used the actual pinned executable with an SDK MCP server and local
model peer, and made two model requests. No live provider was used. The verifier
also rejected a deliberately changed tool correlation ID and incorrect response
framing in each case.

The executable SHA-256 is
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`.
Expected values are in `tests/fixtures/plugins/claude-post-source.json`.
The run retained callbacks and downstream model requests in
`/tmp/demoncoder-post-source-xsmpnp_v`, with content hashes in its `result.json`.
These temporary artifacts support this development observation; the repeatable
source check and fixture are the retained repository inputs.

Observed contracts:

- `PreToolUse` precedes `can_use_tool`, then the SDK MCP `tools/call`, then the
  post-tool callback. Both callbacks and the permission request carry the actual
  model tool-use ID. MCP `params._meta["claudecode/toolUseId"]` carries that same
  ID. The outer SDK control request ID is a different identity.
- The callback tool name is `mcp__demoncoder__capture`. Its required session,
  working directory and transcript fields agree between pre and post events.
- A successful post-tool frame contains the MCP content array as `tool_response`.
  It does not contain the entire MCP response or a decoded host result object.
- An MCP `isError: true` response produces `PostToolUseFailure`, with the returned
  text as `error` and `is_interrupt: false`.
- `updatedMCPToolOutput` containing a text content array becomes that exact array
  in the next model request. Wrapping it inside a serialized host tool result
  would change the observed source behavior.
- A nonempty string replacement becomes string content. The probe compares it
  before the backend's separately appended system reminder. Null, false, zero
  and an empty string are ignored; the original content array survives.
- A plain object replacement makes the pinned backend's post-processing fail
  with a type error and emit an additional `PostToolUseFailure`. The host contract
  requires stricter handling: retain the original successful execution and hold
  invalid presentation without fabricating a failed host operation. Production
  verification must demonstrate that distinction.

The public [hook reference](https://code.claude.com/docs/en/hooks) describes
MCP-only output replacement. The pinned executable probes above establish the
specific transport shapes used by this implementation; current documentation
alone is not evidence for a frozen source version.

## Pinned Codex source observations

`python3 tests/plugin_codex_post_source.py --codex /home/shawn/.cache/demoncoder/managed-codex-v0.153.4/codex-rs/target/debug/codex`
passed actual managed app-server success and failure cases, using a local model
proxy and a single dynamic tool. Each made two model requests. The fixture trusted
only its two exact command-hook hashes in its private configuration. These were
ordinary source hooks; the test did not substitute a host dispatch for them.

The executable SHA-256 is
`80315a32acf1b625129a46b0bd75537cf76ef09701ae986154159076cc0b6aff`.
Expected values are in `tests/fixtures/plugins/codex-post-source.json`.
The run retained source hooks, app-server messages and model requests in
`/tmp/demoncoder-codex-post-source-_krmrd5y`, with hashes in `result.json`.
The verifier rejected corrupted source tool-use IDs in both cases, an incorrect
successful response shape, and an invented successful post event after failure.

- Dynamic-tool success produced `PreToolUse` and `PostToolUse`. The post frame's
  `tool_response` was the single input-text result as a JSON string.
- Dynamic-tool failure produced only `PreToolUse`; no successful post event or
  invented failure event was emitted. The model still received the failed tool's
  text result.
- Source session and turn IDs matched app-server thread and turn IDs. The tool
  ID matched `item/tool/call.params.callId`. Approval policy `never` produced
  source permission mode `bypassPermissions`.
- `thread/start.result.thread.path` exposed the actual rollout path used in the
  source event. A host adapter can preserve that returned optional path without
  constructing a path from an assumed filename convention.

These observations agree with pinned source `core/src/tools/registry.rs`
(`post_tool_use_payload`, `dispatch_any_with_terminal_outcome`),
`core/src/tools/handlers/dynamic.rs` and `core/src/tools/context.rs`
(`function_tool_response`). They qualify the source mapping; production adapter
checks still must exercise the implementation below.

## Production Codex adapter checks

`cargo test --locked --test plugin_post_codex --test plugin_tool_receipts`
passed six tests. The three new controlled app-server cases exercise the production
adapter, shared tool executor and durable runtime. They demonstrate an actual
successful write with retained Codex call/thread/turn identity, returned model and
transcript path; a required post-tool hold that preserves the write and prevents
result delivery; and an admitted read failure without a fabricated successful
post-tool event. Successful delivery records acknowledgment after the transport
write. The existing three receipt tests passed in that run.

A subsequent `cargo test --locked --test plugin_post_codex` passed all five
Codex cases, adding cancellation after the real write while its post-tool handler
waited. Cancellation retained the original successful result and an unfinished,
uncertain hook receipt, and prevented backend result delivery. The fifth case
uses the actual confined CommandRunner and asserts its JSON stdin: source IDs,
model, permission mode, transcript path, arguments and a JSON-string tool result.
Deliberately incorrect configured model and transcript values cannot replace the
actual app-server facts.

Before the adapter change, both new success/hold cases failed because the hook
received `ToolRepresentation::Native`. These checks use a controlled backend,
not an installed Codex process or a live provider. The source observations above
separately establish the field mapping used by the adapter.

## Runtime boundaries under review

The host retains the original tool result before awaiting a post-tool runner.
Post-tool capability keys include the event and the exact reserved inspection;
a pre-tool slot cannot gain authority by reusing its numeric index after tool
completion. A sibling runtime cannot borrow that capability.

Native delivery stays `LocalPending` until the final input comparison and owner
check succeed. External delivery stays `Staged`, becomes `Reserved` before the
transport write, and becomes `Acknowledged` afterward. Interrupted validation or
an unacknowledged external write remains held on recovery. Correction allowance
is consumed only by the guarded release transaction; hook model usage remains
charged even when validation fails. Rejected publication cannot leak a partially
updated task or receipt into a later save.

Verification and child-check callers now request original evidence explicitly.
Their saved check output cannot become a plugin's model-facing replacement, and
they validate continuation before proceeding to another check.

## Scope limits

This prerequisite covers synchronous post-tool dispatch. One-shot consumption,
asynchronous job ownership, other lifecycle boundaries, public activation and the
complete installed/live conformance matrix remain part of this commitment.

The initial implementation held native follow-up on external backends. Independent
review rejected that as incomplete for this bounded task. The frozen correction
candidate now implements backend interruption and replanning under the original
owner and allocation, as recorded below. Independent re-review is pending.
Claude source model outcomes that allow continuation also use the owning task or
supervised assignment's existing correction ledger; they create no new allowance.

A host pre-tool denial is not an admitted host execution. Claude may subsequently
send its SDK failure callback for the MCP error. That callback can acknowledge the
source outcome, but it does not invent a completed host operation or dispatch a
host post-tool handler. The original denial remains retained.

## Remaining verification

Production tests must independently demonstrate real successful writes and
admitted failures, retained original results, separate model presentation,
durable correction counts, held continuation, source capability identity,
cancellation and no replay. Specification and quality reviews follow those
checks. No approval is recorded yet.

## Broader regression checks

The first `cargo test --locked --all-targets` run stopped at three existing
tool-operation unit tests (201 passed). They exposed a regression in UI
backpressure: advisory delivery could drop the completion notice and let legacy
presentation run early. The fix retains the original completion synchronously
and awaits its delivery after model-result retention and observer-free settlement.
All 23 tool-operation tests then passed without weakening their assertions.

A subsequent library run exhausted the system `/tmp` inode pool and failed
filesystem setup. Those failures are not passing evidence. Follow-up runs use an
isolated temporary directory on the home filesystem.

`git diff --check` and syntax parsing of both source-probe Python scripts passed.
Ripwire `--quality-delta` could not establish its Git comparison in this linked
worktree (it reported no baseline and no Git HEAD); this is not a quality pass.
Its `--test-gate` listed 38 test files and 383 symbols without mapped coverage.
The list guides broad regression execution; static reachability is not proof that
those symbols are untested or correct. Independent source review remains required.

After the delivery fix, library tests (204), post-tool tests (22), Codex tests (5),
receipt tests (3), all-target Clippy and formatting passed. The broad rerun with
`TMPDIR` on home still encountered three `/tmp` allocation errors because some
production paths explicitly create directories there. A subsequent broad run uses
a private mount namespace with a home-backed directory mounted at `/tmp`; ordinary
network access and the inner production confinement remain unchanged. This avoids
removing unrelated temporary data. Results are recorded below once complete.

The private-mount run was rejected as verification evidence: its namespace changed
executable and nested-confinement permissions. Running fixtures under a home cache
path also changed cache-mapping behavior, so that diagnostic run is not a passing
baseline. Restored normal `/tmp` capacity by removing 2,451 empty DemonCoder scratch
directories older than two hours after checking live process directory, descriptor,
command-line and environment references. No nonempty directory was removed. The
next all-target run uses the ordinary environment with `TMPDIR` unset.

## First independent specification review

A fresh reviewer inspected the frozen 46-file candidate against the bounded
plan and specification. All candidate hashes remained unchanged during review.
Verdict: changes required; this prerequisite is not complete.

- Native final-release validation awaits outside the cancellation select. A
  cancellation queued after execution can be missed before correction is charged.
- Native follow-up requests on external backends are converted unconditionally to
  a hold. That is safe but does not implement the requested correction behavior.
- Replacement-only model output lacks plugin attribution, despite durable
  proposal provenance. Source-shaped replacements need separate attributed context.

These are implementation findings within the current bounded task. They are not
removed from scope. The reviewer inspected code and assertions without running
Cargo; no independent test pass was claimed. Fixes and re-review are pending.

The ordinary broad test run passed library (204), developer access (15, one
ignored), host guard (10), language services (28, three ignored), and preceding
suites before stopping at plugin admission on renewed `/tmp` inode exhaustion.
The home-cache TMPDIR experiment is unsuitable because that location has distinct
cache mapping. A plain home temporary root, `/home/shawn/demoncoder-check-tmp`,
passed all 15 active developer-access cases with one installed case ignored.

## Qualified external correction boundary

The repeatable pinned-source probes now pass ten Claude cases and three Codex
cases, including interruption while the completed host result is withheld from
ordinary backend continuation. The executable pins are unchanged. Artifacts are
`/home/shawn/demoncoder-check-tmp/demoncoder-post-source-ghowwdvd` and
`/home/shawn/demoncoder-check-tmp/demoncoder-codex-post-source-x9f7qu93`.
Each directory retains the exact probe, fixture, bidirectional trace, local model
requests and hashes. These checks use actual pinned backends with local model
peers; they are source qualification, not production adapter or live-provider evidence.

Claude can interrupt while its actual PostToolUse callback remains unanswered.
The host already supplied the original MCP result. Claude acknowledges the
interrupt and emits `terminal_reason: aborted_tools` with an error result; the
probe accepts that terminal form only for this correlated interruption. No second
model request occurs before both signals and a new user turn. The same process
and session accept the correction. With the pinned `--replay-user-messages` flag,
Claude echoes the exact host-supplied UUID and message with `isReplay: true`.
The echo precedes the new response on stdout. A provider HTTP request can arrive
before the host reads that stdout echo; the new invocation must already be
reserved before sending its prompt. Corrupting either interrupt correlation or
that user UUID makes the verifier fail.

Codex can interrupt while the dynamic-tool response remains unanswered. The host
waits for the exact interrupt reply and an interrupted completion for that turn.
Only then does it send another turn on the same thread. The second model request
contains the correction and retained host completion; no old successful post event
is invented. Corrupting the interrupt acknowledgment makes the verifier fail.

Both probes retain and distinguish backend cancellation placeholders from the
actual host result. Correction context explicitly says the host interrupted for
plugin follow-up; it does not portray that as a developer denial or undo the
completed operation. The selected production protocol is recorded in the
[external correction decision](../decisions/supersede-external-backend-turns-before-post-tool-correction.md).
Production implementation and failure-path verification are still in progress.

## Supervised child correction ownership

A focused independent follow-up found that requiring the literal `worker` phase
is too narrow for an explicitly admitted supervised child post event. The existing
worker phase is `agent:<id>:worker`. Its correction owner is that assignment's
`OrchestrationState.correction_rounds`, with the retained orchestration limit;
`record.task.corrections` belongs to the parent and must not be charged instead.
Shared model, tool and backend allocations still apply. Exact child identity,
active assignment and parent ownership must be checked. Unsupervised children
have no correction ledger and remain held; checking, advisor, judge, verification
and hook phases cannot gain worker authority. Hook follow-up cannot discharge
supervision findings or authorize integration.

This is a same-owner accounting gap within the synchronous prerequisite. It does
not authorize public child plugin inheritance: the current manager deliberately
rebuilds child policy with `worktree_only`, and activation wiring remains later
work. The review inspected existing manager, state and supervision code without
changing files, running tests or approving the overall candidate. Correction and
focused capability tests are in progress.

## Retained source evidence integrity

A later integrity check parsed both source-probe scripts and compared their bytes
and fixture bytes with the retained copies in the two home-backed evidence
directories above. All 30 Claude artifact hashes and all 12 Codex artifact hashes
matched. The recorded runs contain ten and three cases respectively. This check
confirms those artifacts still describe the current probes; it is not a new
backend execution or production-adapter pass.

## Codex correction acknowledgment ordering

The retained pinned Codex correction trace places the original turn-start RPC
reply at row 10 before its start notification at row 12, and the correction reply
at row 33 before its notification at row 35. That observed order is not a
protocol guarantee. In the pinned source,
`app-server/src/request_processors/turn_processor.rs:626` starts the core turn
before returning the RPC response at line 686. The separately spawned listener
in `request_processors/thread_lifecycle.rs:282` consumes core events and invokes
`bespoke_event_handling.rs:166`, which sends TurnStarted through the outgoing
channel. The notification sender has no pending-client-reply barrier.

The production adapter must tolerate a bounded early start notification while
awaiting the exact RPC reply. The notification alone cannot acknowledge a new
correction or authorize tool execution. The implementation and targeted ordering
controls are being completed before freezing the candidate for review.

## Frozen correction candidate

The candidate at base `37f609a3c5de0abcd55556edab246294164313fb` now includes
cancellation ownership around native final validation, separate plugin attribution
for source-shaped output replacements, and bounded external correction on both
backends. Child corrections charge only the retained supervised assignment ledger
and preserve findings; the parent task's corrections remain unchanged. The Oracle
request retains authentic developer intent rather than the plugin correction text.

After the latest fixes, `cargo test --locked --lib --test plugin_post_tool
--test plugin_post_codex --test plugin_tool_receipts` passed 209 library, 28 post-tool,
six Codex and three receipt tests. All-target Clippy with warnings denied and
formatting passed. The parent inspected the retained logs and separately passed
`git diff --check`. Independent specification re-review and the ordinary
`cargo test --locked --all-targets` run remain in progress. No approval is claimed.

A separate ordinary checkout at the same base received exact copies of changed
source and test files for Ripwire comparison. `--quality-delta` returned exit 2:
298 findings, including 105 gating rows. These include substantial adapter-loop
complexity growth, duplication, new interfaces and heuristic dead-code/churn
findings. They are not a static pass; the quality review must assess the real
maintenance risks and distinguish folded same-name or trait/test discovery limits.
`--test-gate` returned exit 4 and named 39 test files plus 397 symbols lacking mapped
coverage. The full Rust suite covers the active Rust cases; installed/live and
script entry points still require their separately declared execution.

## Specification re-review: child supervision state

The independent reviewer found an additional production-reachable child state
that the new owner check rejects. The manager marks the preceding worker turn
completed before supervision; a judge-admitted correction restores Running and
Correcting without clearing that prior `completed` flag. Requiring
`!child.completed` rejects the corrective worker even when its assignment retains
one correction round. The new fixture covers only initial Working with
`completed=false`. This finding remains open; source review continues against the
frozen candidate before a separate fix and regression.

The ordinary all-target Rust run completed with exit 0: 651 tests passed, none
failed and 16 installed/live/subprocess cases remained explicitly ignored across
41 suite results. The parent verified all 54 candidate hashes afterward. This
regression result applies to the frozen candidate and does not resolve the child
supervision finding above. The log is retained at
`/home/shawn/demoncoder-check-tmp/post-all-targets-final.log`.

The child-state regression failed before the repair. The owner check now permits
retained completion only in an active Correcting stage with an already admitted
round. The regression spends the child's final remaining round through both
local and external release, preserves findings and the parent ledger, and rejects
a third round. Completed initial work, Correcting without admission, stopped and
Checking states remain rejected. Only the owner helper and its tests changed.
The three focused child tests, all 246 affected tests, Clippy and formatting passed
afterward. Independent final specification review is pending.

## Final bounded specification verdict

The independent specification reviewer approved the corrected candidate after
checking the actual supervisor state transition, owner helper and regression
assertions. The reviewer independently ran all three focused child lifecycle tests;
all passed. The 54 candidate hashes matched before and after review. The original
three findings and the subsequent child-state finding are resolved for this bounded
prerequisite. A fresh independent quality reviewer is examining the same candidate,
including the static complexity and duplication findings. No quality approval or
full-commitment completion is recorded yet.

## Independent quality review: silent correction acknowledgment

An independent temporary probe against the frozen production Claude adapter
confirmed that a silent backend after the correction user message is sent has no
short acknowledgment deadline. The probe's 32-second outer timeout fired while
the durable state remained CorrectionReserved, with one correction charged and
the successful original result retained. The ordinary workflow limits this wait
only by the remaining task allocation (900 seconds by default). This is an open
quality finding; the candidate remains frozen while the reviewer finishes other
checks. The reviewer retained its harness and log in
`/home/shawn/demoncoder-check-tmp/post-quality-probe` and
`/home/shawn/demoncoder-check-tmp/post-quality-silent-probe.log`.

The noisy-peer probe separately reproduced Codex deadline starvation: 35.963
seconds after the peer received the interrupt, only the probe's 36-second outer
timeout stopped the adapter. Claude's corresponding noisy probe returned its
30-second timeout, so the observed failure is specific to Codex; source similarity
alone does not establish the same failure on Claude.

A third independent probe supplied an invalid Claude MCP object replacement.
The runtime correctly held continuation and retained no model replacement, but
its durable proposal disposition incorrectly said Applied. That is a separate
audit-record defect. The reviewer is completing existing regression checks and
its final hash comparison before these findings become a separate repair action.

The completed quality review requires those three Important repairs and found no
Critical issue in the inspected scope. It independently passed five existing
external correction tests and confirmed all 54 hashes remained unchanged. The
review explicitly assessed the static findings: adapter timing needs a small
shared stage/deadline contract; the native ownership sequence, source-specific
framing and separate pre/post capability checks do not need broad rewrites. Trait
and test discovery limits, same-name folding and fixture forwarding account for
many heuristic rows. The static command still did not pass.

These findings are recorded before repair. The next implementation action will
add effective stage-specific transport deadlines on both external adapters and
make rejected replacement dispositions agree with actual application, with
failing-before/fixed-after production regressions and independent re-review.

During the timing repair, parent source inspection also found that both adapters
started the stage clock after awaiting the correction write, and started the
supersession clock after writing the interrupt. A peer that stops reading can
therefore block a large correction before that clock exists. The same timing
repair must establish the stage before transmission and bound writes as well as
reads, preserving uncertainty after any partial transmission. A blocked-reader
regression is requested before the next freeze.

The blocked-reader experiment corrected the parent's initial interpretation:
`BackendProcess::send` already wraps its write in a ten-second timeout. The
unmodified send failed with `backend input timed out` after about eleven seconds;
it was not an unbounded write. The three independently confirmed quality findings
remain unchanged. Starting the shared stage clock before transmission still
prevents a write from extending an already-running stage; the shorter existing
write timeout remains in force. This experiment is a passing existing safeguard,
not a claimed failing-before/fixed-after defect.

## Supplemental specification finding: structured replacement content

The three quality repairs passed 253 affected tests (211 library, 33 post-tool,
six Codex and three receipt tests), Clippy and formatting. They are frozen for
review; the blocked-write experiment confirms the existing ten-second safeguard.

A separate pinned Claude probe found that a provider-shaped base64 image in
`updatedMCPToolOutput` reaches the next local model request unchanged and passes
the existing correlation/framing verifier. The current text-only host validator
rejects that shape. Independent specification review found no text-only exemption:
source-supported replacements belong to this bounded task. This supplements the
earlier approval, so the prerequisite remains incomplete.

The raw MCP image shape instead failed before a second model request with
`undefined is not an object (evaluating 'e.source.type')`. A resource-shaped block
was forwarded to the local peer, but that peer accepts JSON; forwarding alone does
not establish provider validity. These results are source observations, not live
provider evidence. The retained traces are under
`/home/shawn/demoncoder-check-tmp/post-extra-block-xw99on6c`. Provider content shapes
and bounds are being qualified before a focused representation repair.

## Structured source qualification

The pinned backend preserved provider-shaped base64 and URL images, plain-text,
base64 PDF, URL and nested-content documents, and search results in subsequent
local model requests. The fixtures contain a generated PNG and a minimal PDF;
the local peer does not validate media decoding or fetch remote URLs. These
observations are retained in the expanded Claude source fixture and probe.

The verified official `@anthropic-ai/claude-agent-sdk` 0.3.267 archive declares
provider SDK `>=0.93.0`, not an exact bundled dependency. Official provider SDK
0.93.0 types supply a reproducible schema baseline (archive SHA-256
`bf747fe9ab5922edf2a6bc7871927ea525fae9239ec384a3092e7d8023ee88b9`). They identify
text, image, document, search-result and conditional tool-reference families.
That baseline is not a claim about the executable's internal dependency version.

Search-result content must contain only search-result siblings. In the actual
source probe, hook `additionalContext` becomes a separate text block next to the
tool-result envelope, preserving the search-only content. The host repair must
also enforce citation-setting agreement with previously delivered search results
in the same source conversation, including prior turns and different roles.
The check must run again inside release reservation so conflicting staged outputs
cannot both pass. Reserved results conservatively count as possibly delivered.
Retaining this constraint across compaction can hold a later setting change after
the old result has left model context; receipts alone do not reconstruct the exact
next provider request. Historical correction prompts contain serialized text and
do not count as delivered structured search results. Typed corrections need the
representation distinction and release checks described below.

A known tool reference is not preserved in the qualified backend configuration.
Although both model requests advertise the referenced tool, Claude replaces the
reference with `[Tool references removed - tool search not enabled]`. A valid
provider type and matching definition therefore do not establish source support.
The host must retain the original result and report the unavailable source
capability, rather than claim application or call valid provider content malformed.
Enabling and qualifying tool search remains a full-commitment obligation.

Synthetic image/document file IDs are forwarded unchanged, but the captured
request headers do not include `files-api-2025-04-14`. The probe retains only
the request path and allowlisted protocol headers, never credentials. It makes no
claim that the synthetic files exist or that a provider accepts them. File-source
capability remains unqualified; no upload or beta activation is implied. This is
a recorded compatibility obligation, not a permanent scope exclusion.

The first expanded source check passed all 21 cases and made 42 local model
requests. Its verifier rejected changed downstream content in every ordinary
case and deleted attribution in the search-context case, as well as the existing
correlation, framing and correction-acknowledgment mutations. The parent checked
all 84 artifact hashes and exact retained probe/fixture bytes. Evidence is kept at
`/home/shawn/demoncoder-check-tmp/demoncoder-post-source-n1ua7w92`.
An earlier new negative probe exposed an unchecked verifier string assumption;
explicit type assertions fixed that verifier failure before the passing rerun.

## Structured replacement with correction

The implementer identified a second representation interaction before freeze:
the correction helper serializes replacement output into its prompt. A valid
image, document or search result combined with a correction request therefore
loses its semantics. Independent specification review confirmed that this is
part of the same bounded replacement contract.

Three additional actual pinned probes preserved image, nested document and
search-result blocks in typed corrective user messages after the existing
interruption barrier. The SDK echoed the exact typed message and UUID; the
following local model request retained the content, with backend cache metadata
allowed. Top-level user search results may appear alongside attributed text;
the homogeneous-content rule applies inside tool-result envelopes. No new
tool-result envelope or reused cancelled tool ID is required.

The [recorded implementation choice](../decisions/preserve-structured-claude-content-through-post-tool-corrections.md)
keeps host evidence separate, preserves the same owner and allowance, and binds
acknowledgment to the exact typed message. Typed correction delivery must have
an explicit receipt representation so historical JSON-text corrections keep
their meaning. New typed CorrectionReserved and CorrectionAcknowledged results
participate in citation agreement, including atomic revalidation when reserving
the correction. Uncertain transmission remains held and cannot replay.

The implementation and production regressions are in progress. The source probes
remain local-peer qualification, not live-provider acceptance.

The initial typed probe passed 24 cases with 48 local model requests, including
all three typed corrections. Deleting a corrective semantic block from the next
model request makes the verifier fail even when its SDK echo remains intact.
The parent verified all 96 artifact hashes and exact probe/fixture copies at
`/home/shawn/demoncoder-check-tmp/demoncoder-post-source-of5rmeci`.

An additional empty-array probe distinguishes source truthiness from output
normalization. Claude applies `[]`, removes the original model presentation, and
emits `(mcp__demoncoder__capture completed with no output)` before its own reminder.
The host preserves the empty callback array and original execution receipt; it
does not claim downstream array preservation in that case. The latest source
run passed all 25 cases and 50 local model requests. All 100 artifact hashes and
exact current probe/fixture copies matched at
`/home/shawn/demoncoder-check-tmp/demoncoder-post-source-a8vtkkiu`.

## Frozen structured-content candidate

The final affected run passed 270 tests: 222 library, 39 post-tool, six Codex
post-tool and three receipt cases. All-target Clippy with warnings denied and
formatting passed. Two controlled mutations failed as intended: removing atomic
citation validation admitted a conflicting release, and serializing corrective
blocks into text failed the typed delivery oracle. Both source files matched their
pre-mutation hashes before the passing run. The parent inspected the retained
test and Clippy logs.

The renewed review manifest fixes 58 file hashes at base `37f609a3c5de0abcd55556edab246294164313fb`.
The broader Rust run passed 675 tests, with zero failures and 16 explicitly
ignored installed/live cases across 41 suite results. The parent confirmed all
58 hashes afterward. This baseline does not resolve the envelope-size finding
below; independent specification review is still in progress.
Ripwire on exact copies in the ordinary comparison checkout returned exit 2 with
340 findings and 105 gating rows; its test gate returned exit 4 with 39 test files
and 427 symbols lacking mapped coverage. These are not static passes. The known
same-name folding and trait/test discovery limits remain relevant, and the quality
review must assess the actual new validation and delivery code after specification
approval.

## Structured-content re-review: transport envelope size

The specification reviewer confirmed a boundary mismatch: corrective content may
approach 4 MiB, while the backend receive limit applies to the entire echoed JSON
line. A typed content array of 4,194,304 bytes passes preparation, but its exact
replay has a 4,194,475-byte lower bound. A separate reachable string witness uses a small write,
a small replacement, and 2 MiB of backslash-containing developer steering. Its
2,097,882-byte prompt passes, but its serialized replay is at least 4,195,234 bytes. An
earlier read-output witness was replaced because it could hit an earlier source
callback bound and therefore did not establish this handoff path.

Both exceed the 4 MiB receive cap after the correction has been reserved. The
repair must budget complete serialized message and replay framing, including
escaping and steering, before reservation and charging, for both string and typed
corrections. Witnesses are retained in the `provider-types` evidence directory.
The review was recorded before unfreezing the candidate for a separate repair.
The old 58-file manifest is now historical; a new freeze and both reviews are
required after the repair.

Pinned Claude source inspection and a renewed actual-source run established that
the replay also adds a timestamp. The probe now sends the production envelope's
parent and source-session fields, checks the complete replay field set and its
ISO timestamp, and rejects changed timestamps or extra envelope fields. All 25
cases and 50 local model requests passed at
`/home/shawn/demoncoder-check-tmp/demoncoder-post-source-3vx6_y6e`.
The timestamp explains why the earlier witness sizes are lower bounds.

Pinned Codex traces expose a corresponding boundary: `item/started` and
`item/completed` notifications both contain the complete corrective user text.
Their framing must fit before correction admission even though acknowledgment
uses the separate RPC response. The production regression reproduced an
oversized notification after charging; request and notification preparation is
being repaired together. Neither transport limit is being increased.

The strengthened actual Codex probe passed all three cases and six local model
requests at `demoncoder-codex-post-source-rp8b6zgm` under the same evidence
directory. It checks the complete started/completed notification shapes and
corrective text, generated identifiers and timestamp bounds. Corrupting the
echoed content, adding an envelope field or replacing the timestamp with text
each fails verification. The parent verified all 12 artifact hashes and exact
retained probe/fixture bytes. These source checks remain controlled local-peer
qualification and do not replace production-adapter tests or live evidence.

## Frozen complete-frame repair

Both adapters now prepare the complete outgoing request before correction
reservation and transmit that same value. Claude budgets its qualified replay
including the longest constructor timestamp. Codex budgets both complete
user-message notifications as well as the request. All use the unchanged shared
4 MiB receive limit. Failed preparation retains the original result without
charging, creating another invocation or recording a delivery marker.

The final affected run passed 276 tests: 224 library, six Codex post-tool,
43 post-tool and three receipt cases. All-target Clippy, formatting and diff
checks passed. Baseline regressions reproduced charged oversized string
handoffs on both adapters; a controlled restoration of the old typed-content
bound also failed as intended. Corrected cases cover escaped and multibyte
content, exact accepted boundaries, envelope overflow, retained effects and
no replay. Logs are `frame-final-tests.log`, `frame-final-clippy.log` and
`frame-final-fmt.log` in the evidence directory. The parent inspected them.

The new review manifest freezes 59 files at the same base. The previous 58-file
manifest is retained separately. Independent specification review is running
against this corrected candidate; quality approval remains pending.

Ripwire on the matching comparison checkout reports 348 findings, including
106 gating rows (exit 2). Its test gate names 39 test files and 411 symbols
without mapped coverage (exit 4). These results are retained in
`post-envelope-quality-delta.{log,json}` and `post-envelope-test-gate.log` for
the quality review; they do not establish either a static pass or actual absence
of test coverage.

## Complete-frame review: pre-acknowledgment buffering

Independent specification review reproduced one remaining failure in a temporary
production-adapter harness. Moving the exact full-text Codex user notifications
before the correction RPC reply makes the near-bound positive fail with
`Codex pre-acknowledgment frames exceed bound`. One correction was charged and
the handoff remained CorrectionReserved; the original successful write survived.
A small correction with the same ordering succeeds. The separate 1 MiB cumulative
pre-acknowledgment buffer is smaller than the validated individual frames.

Pinned source starts the core task before building the turn-start response.
A separately spawned listener can send its notifications through the same output
sender without waiting for that response. Source therefore permits this ordering;
the reviewer demonstrated it with controlled transport, not an observed natural
pinned-backend run. The completed review confirmed all 59 hashes and found no
other bounded issue. A separate repair is now in progress; the old manifest
remains retained as the reviewed candidate. No pass is claimed for this finding.

## Frozen pre-acknowledgment repair

Known no-ID Codex user-message start/completion notifications now retain their
method and required thread/turn identity in the original ordered queue without
the text that normal dispatch ignores. The exact RPC response still owns
acknowledgment. Other messages retain their prior handling. The 4 MiB receive
limit, 1 MiB queue limit, 64-frame count and transport deadlines are unchanged.

The new production regression failed before this repair and passed afterward.
It covers large messages before and after the reply, early tool/completion events,
wrong or missing correlation, unknown/request payloads, queue bounds and
cancellation. Large notifications without the RPC reply still reach the real
acknowledgment timeout and remain uncertain without replay.

An initial six-peer deadline fixture hit the existing input-write timeout before
reaching its intended acknowledgment condition. The same new large-input cases
passed separately, each reaching the 30-second deadline. They now run sequentially
in a separate test under the existing global fixture lock, which prevents overlap
with the original noisy-peer composite. No production timeout was relaxed.

The final affected run passed 279 tests: 224 library, six Codex post-tool,
46 post-tool and three receipt cases. Clippy, formatting and diff checks passed.
The parent inspected `preack-final-{tests,clippy,fmt}.log`; diagnostic and
isolated-case logs remain in the same evidence directory. The renewed 59-file
manifest is frozen for the same specification reviewer, followed by quality review.

The refreshed static report contains 355 findings and 107 gating rows (exit 2);
the test gate still names 39 files and 411 symbols without mapped coverage
(exit 4). `post-preack-quality-delta.{log,json}` and
`post-preack-test-gate.log` retain those nonpassing results for review.

## Final bounded specification approval after buffering repair

The same independent specification reviewer approved the corrected prerequisite.
Its original external production regression now passes, along with two current
production tests. All 59 candidate hashes and 112 source artifact hashes matched.
All reported bounded specification findings are closed. The same quality
reviewer is now examining this candidate and the retained static findings.
The full commitment remains in progress.

## Quality re-review: auxiliary writes can overrun the handshake deadline

The quality reviewer reproduced another bounded transport failure against the
frozen candidate. A controlled Codex peer waits 29 seconds after the interrupt,
sends a valid roughly 512 KiB unknown RPC with a large string ID, then stops
reading. The fallback response uses the existing ten-second write timeout but
awaits outside the active correction deadline. The outer probe timed out at
35.913 seconds after the interrupt with no adapter error yet.

The receipt remained Superseding, zero corrections were charged, the original
successful result survived, and no next turn began. This demonstrates response
write backpressure extending the intended 30-second handshake, not loss of tool
evidence or a slow relay task. The reviewer is checking the equivalent Claude
control response and eventual write error before closing the review. The candidate
remains frozen; this finding is recorded before any repair.

The completed quality review confirmed the same failure on both adapters.
Their eventual errors arrived 39.014 seconds (Codex) and 39.021 seconds (Claude)
after interruption: `backend input timed out: deadline has elapsed`. Both kept
Superseding state, zero charged corrections, the original write, and no next turn.
The original three quality findings are closed; 18 targeted tests passed and
all 59 hashes matched. This is the sole remaining Important finding. The review
is retained at `plugin-post-quality-final.md` in the evidence directory.
A separate repair will apply the active deadline to handshake-owned dispatch
waits and replies, followed by renewed specification and quality review.

## Frozen dispatch-deadline repair

The shared deadline now tracks the current stage across received-message and
relay dispatch, auxiliary replies, reservation and reserved context publication.
It preserves one absolute deadline while a stage is active and observes timely
acknowledgment clearing without retaining an obsolete timer. Expired stages
cannot be revived. Ordinary work without an active stage retains its behavior;
the shorter write timeout and all transport/queue limits remain unchanged.

The six delayed dispatch scenarios passed in 30.99 seconds: both adapters'
auxiliary replies and refused tools, plus Codex relay read and authenticated
handler waits. The held relay lock is released on timeout. Six clock/frame tests
passed, including expired-before-poll, live acknowledgment and activation,
error preservation and dropping lock-holding operations. Both blocked Context
publication cases passed in 30.60 seconds with no request sent, reserved
uncertainty and retry refusal. The fixture drains its deliberately full UI queue
after establishing the primary timeout before checking that refusal.

An initial full run exposed a hidden wrong-thread diagnostic: correlation still
failed, but the new error context obscured its reason. The repair preserves the
underlying display text and error chain. Its focused regressions and the final
affected suite passed: 283 tests (226 library, six Codex, 48 post-tool and three
receipt cases). Clippy, formatting and diff checks passed. The parent inspected
`dispatch-final-tests.log` and `dispatch-final-clippy.log`. The 59-file candidate
is frozen for renewed specification and quality review.

The refreshed static report contains 363 findings and 107 gating rows (exit 2).
The test gate names 39 files and 413 symbols without mapped coverage (exit 4).
These remain nonpassing heuristic results in `post-dispatch-quality-delta.{log,json}`
and `post-dispatch-test-gate.log`. The broader Rust suite will also be rerun
because the dispatch wrapper now affects ordinary adapter control flow.

The independent specification reviewer approved the repaired prerequisite after
examining the shared clock, acknowledgments, ordinary dispatch and timeout cleanup.
Its broad `cargo test --locked --all-targets` run passed 688 tests, with 16
explicitly ignored cases across 41 suites (exit 0, 454.28 seconds). The result is
retained in `post-dispatch-all-targets.log`. All 59 candidate and 112 source
artifact hashes matched. No bounded specification finding remains. The same
quality reviewer is now rerunning the delayed-response probes on this candidate.

## Final bounded quality approval and self-audit

The same independent quality reviewer approved the corrected prerequisite with
no unresolved Critical or Important finding. Its original delayed-response
probes now return the supersession timeout at 30.005 seconds (Claude) and
30.002 seconds (Codex), retaining original success, zero charged corrections
and no next turn. Six independent clock/frame tests passed; all 59 frozen
hashes matched. The report is `plugin-post-quality-dispatch-final.md` in the
evidence directory. Static findings retain their recorded dispositions and
nonpassing exits.

The parent checked the bounded implementation against the production standard:
confirmed scope, coherent boundaries, error and cancellation behavior, retained
evidence, legacy state, confinement, limits, regression proof and both reviews.
No unresolved bounded defect remains. Only this prerequisite is complete; the
plan retains one in-progress lifecycle item and the full commitment obligations.
The final plan status and this review record are the only documentation changes
after the reviewed source freeze.

## Codex source helper regression during async qualification

The optional async scenario in `tests/plugin_codex_post_source.py` preserves its
original success, failure and correction cases. All three passed with their
original corruption checks at `/tmp/demoncoder-codex-post-source-1ex6mc18`.
The captured helper SHA-256 is
`29a3866c3fef0639dcc75c5a73ed4f6a46bdd8425c20a8753af93b8d36429b5b`.
The parent and both independent source reviewers checked its 12 recorded artifact
hashes and replayed the original verifier against those cases. The reviewers did
not rerun the source executable. See [the async source review](plugin-codex-async-source.md)
for the new source observations and limits. Earlier evidence above remains historical.
