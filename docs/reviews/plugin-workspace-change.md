# Admitted workspace replacement

Status: Implementation in progress; application qualification pending. This is a bounded prerequisite
of the existing skills/plugins/hooks commitment. Complete lifecycle dispatch is
the sole task in progress. No application qualification is claimed.

## Contract and code discovery

[The decision](../decisions/own-admitted-workspace-replacement-through-the-original-session.md)
owns a developer-selected idle workspace replacement, preserves the original
allowance and relevant conversation, and records actual post-change observations.
PCOMP-003's ownership table assigns admitted workspace changes to the host on
all four connections. CwdChanged is an observation after access validation; there
is no PreWorkspaceChange event. Shell cd alone is not the host operation.

The current application has no public live workspace-change route. Root authority
is retained by ToolExecutor descriptors, workflow/runtime state, Settings controls,
Manager child admission, providers and the terminal. Original native lifetime
validation is also root-bound. A path setter would leave stale authority. Discovery
and a separate contract assessment are retained under
`/home/shawn/demoncoder-check-tmp/workspace-transition-discovery.md` and
`workspace-contract-review.md`. These are read-only assessments, not tests.
The parent resolved their suggested control/accepted-task UX choices within the
existing specification; no narrower commitment or scope change was created.

## Pinned Claude control observations

The exact installed Claude 2.1.267 executable has SHA-256
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`.
The fetched SDK 0.3.267 archive matches the profile's existing hash
`4177598847f37041aadcfe1a922a0153e2e58495100521233fc587e1c0839485`;
its sdk.d.ts also matches the recorded profile. No source code was copied into
the deliverable. The archive and inspected SDK files remain in scratch
`workspace-transition-sdk-0.3.267/`.

The source probes used disposable homes, directories, synthetic authentication,
empty setting sources, disabled tools, strict empty MCP configuration and a
controlled loopback HTTP peer. No provider prompt was sent and the peer recorded
zero requests. This is not live-provider or packet-level network isolation proof.

- `register_repo_root` registers a strict child of cwd or a launch-time additional
  directory. Self, outside and duplicate requests fail without another callback.
  The settled run receives two genuine DirectoryAdded callbacks after successful
  registration responses. A deny/block/continue-false observer reply cannot undo
  registration; the later duplicate remains refused.
- `set_cwd` returns transport success even for nested rejection or needs_trust.
  Explicit accepted trust produces actual cwd changes, independently observed
  through `/proc/<pid>/cwd` and relative read_file returning unique B contents.
  Same-root requests report changed:false. Two settled changes and a canary
  variant produce no CwdChanged callback. This is limited negative evidence for
  these controls, not a claim that every Claude CwdChanged trigger is absent.
- Embedded source inspection identifies a separate shell-tracking callback path
  and shows that set_cwd rehomes settings/hooks/skills, memory and plugins/MCP.
  That static inspection is not execution evidence for every branch. Canaries
  stayed inactive with setting sources disabled, but arbitrary package loading
  and destination memory reads were not qualified. The application decision
  therefore uses controlled replacement rather than live set_cwd.

The initial matrix `workspace-claude-source-matrix-20260913T142413841096Z` has
14 completed steps, but lacks settled trusted-transition observations. The
14-step settled matrix is `workspace-claude-source-settled-20260913T142434087128Z`.
The canary run `workspace-claude-source-canary-20260913T142541380126Z` retains a
harness failure on a legitimate commands_changed system message; this is not a
backend failure. The corrected 14-step run is
`workspace-claude-source-canary-fixed-20260913T142602070061Z`. Raw inputs, scripts,
sent/observed messages, results, canaries, stderr and cleanup results are retained.
All processes exited after cleanup. Stderr files are empty; the probe's file sink
was not byte-capped, a harness limitation.

The parent verified retained script hashes, completed/failed run distinctions,
actual root changes and relative reads for settled cases, duplicate/no-op behavior,
callback counts and no submitted model prompts. The parent audit is
`workspace-source-parent-audit-20260913T142949890317Z.json`, SHA-256
`9aa8b828fe942293d2e50c38aa018af49d037b6f32f555aeef06116adbbbfbf8`.
It binds every retained regular file in all four runs. The first parent audit
incorrectly applied trusted-transition assertions to the initial matrix and
failed before writing; the corrected audit retains that distinction.

The contract assessment confirms that truthful host old/new facts may use the
supported Claude wire format, like the existing host ConfigChange mapping.
Such delivery must retain host provenance and never claim an SDK callback.
Missing set_cwd callbacks do not themselves require changing the profile or
requesting a scope exception. Required conversation continuity and source-specific
output effects remain requirements. Dynamic watch installation and DirectoryAdded
are still subsequent work; parsed/unapplied watches cannot count as completion.

## Verification still required

Application implementation, failing behavioral controls, focused regressions,
independent specification and quality reviews, full regression and exact candidate
integrity checks remain required. Source probes above establish none of those.

Retained workspace-transition-discovery.md SHA-256: `0f76b34c64ee78e79c07160474b456fe64eb549eb54f3f04e23074e662e7030d`.

Retained workspace-contract-review.md SHA-256: `db449262f18efef54e1a69b8052e7ca383684f4c625e5d6623b079c2d5a19ed1`.

Retained workspace-claude-source-report.md SHA-256: `fbd885d8ab711111a12f9eaec3f13f6c9d798bfc338d990b9f3f42e478a31ca0`.

Retained workspace-claude-source-audit.json SHA-256: `ee24e08a1d59233cc13aad0ea580610b5bb6e49714a5993162c49c540c3d5a4f`.

## First implementation control

The first focused RED checks that `/workspace /tmp/new root` is recognized as a
busy-session control. `workspace-change-red-control-recognition-filter-20260913T144156514780Z`
ran one library test and failed the expected assertion, exit 101. The parent
verified log SHA-256 `f596c62019fdfe9b25149b681a44f4b871744d98cf3020f8058adaa17bffdbf8`.
An earlier exact filter selected zero tests and is not a RED demonstration.
The initial `workspace-change-candidates-20260913T144053213379Z.json` is a hash
inventory, not retained source copies; later capture must state its real timing.
This control does not yet prove actual root replacement or conversation continuity.

The exact control-test and module-registration source bytes were subsequently
retained in `workspace-change-source-capture-20260913T144317808254Z/`. Its manifest
records the actual timing: post-RED, before the next edit. The parent independently
verified both copied files against their recorded byte counts and SHA-256 hashes.

## Early focused checks

The typed owner/event/control run
`workspace-change-green-durable-owner-occurrence-20260913T144411133690Z`
passed three executed tests, log SHA-256
`8087c601811381b0fcb0b31220a079da5a21999e0eaa16b159dd5183c0237ef0`.
The root-lineage run
`workspace-change-green-durable-root-lineage-final-20260913T145123008299Z`
passed three executed tests for accepted-task archival and original allocation,
retarget rejection after teardown, and exact applied generation/original owner.
Its log SHA-256 is
`9c5d7ef8dd312123a06d64d7bc073d1fd60fb46549d9ed4437fc4f78f59dac71`.
The parent verified both raw logs and metadata. These are intermediate candidates;
they do not establish actual provider replacement, terminal behavior or final
qualification. The earlier root-lineage missing-symbol compiler failure is a
compile-negative control, not fifteen executed behavioral failures.

The parent also found that existing WorkflowSession::work records its learning-
context-augmented prompt under the developer role. The implementation must account
for this existing provenance limitation when carrying old messages and must avoid
recording generated handoff text as a new developer message. This observation was
sent to the worker before handoff implementation; resolution remains to be checked.

## Preliminary implementation findings

These findings were sent to the worker during implementation. They are not an
independent specification or quality approval. Final source and evidence must
establish their resolution.

- Move workspace dispatch outside resume-only inspection; resolve later relative
  selections against the current admitted root.
- Block child admission and stale Settings publication before recording the
  transition. Prepare fallible bindings before application and retain an applied
  hold if later publication or observation fails.
- Bound cancellation/shutdown while an old provider never finishes closing.
- Preserve pending handoff across controls and no-ops; normal turn completion
  alone cannot establish delivery when a submission gate refused the request.
- Explain legacy composite transcript provenance and never record generated
  handoff framing as new developer input.
- Assert continuity on individual post-change provider requests. Searching all
  requests could pass merely because the initial request contained the text.
- Exercise actual installed-owner file effects; process cwd alone is insufficient.
- Keep test-only private APIs private when splitting installed fixtures.

The strengthened installed Claude/Codex run
`workspace-change-installed-input-effect-green-20260913T152626517215Z` passed one
executed test containing both pinned owners, with `CARGO_BUILD_JOBS=12`. Its log
SHA-256 is `1969b27e6bb0301dcd9be9e0eb9b42c861c76865b1bc58377b1a82c4d9f08d29`,
independently checked by the parent. Raw successful fixtures remain in scratch
`workspace-change-claude-vunEW4` and `workspace-change-codex-b8k6yC`. The first
missing-write failure lost its temporary raw fixture; its captured test log
remains. Later diagnostic fixtures `workspace-change-claude-BGHdTV` and
`workspace-change-claude-47rzWz` retain the wrong-name/repeated-tool and overly
broad request-selector failures respectively. Do not reconstruct missing files
as original evidence. These intermediate results still require final qualification.

A further preliminary finding rejected the intermediate SessionEnd test
`workspace_replacement_makes_original_session_end_observation_unavailable_but_cleanup_finishes`.
It expected unavailability after merely moving A-to-B while A remained unchanged.
That is not the decision: original SessionEnd must still run in unchanged A.
Only physical replacement/rename or lost identity of A can make it unavailable.
The worker was asked to repair ownership validation and demonstrate both actual
positive original-A execution and separate physical-root replacement with bounded
cleanup. The earlier passing assertion is not conformance evidence for this rule.

Further preliminary review required retained completed tool evidence as well as
conversation in the external handoff. A messages-only handoff could omit a read
result absent from the final response. The worker added attributed, bounded tool
evidence and an A-only read canary checked in the exact next B provider input.
Controls must leave that handoff pending. Pre-provider refusal and uncertain
provider delivery must remain distinct; a completed host turn alone proves neither.

The final focused run initially reported 20 passes and two failures in
`workspace-change-focused-lib-workspace-change-final-20260913T165040090670Z`.
The close test measured setup time rather than the declared close grace; its
repair measures from actual close while bounding the whole turn separately.
The runner failure exposed handoff binding applied to hook-model requests.
The repair excludes the hook operation phase, including the shared Prompt/Agent
`for_hook_model` path. These failures and their diagnostic runs remain retained.

The parent checked command exits, nonempty results, log hashes and the recorded
12-job cap for these later development results:

| Scratch prefix | Executed result | Log SHA-256 |
|---|---|---|
| `workspace-change-focused-lib-workspace-change-green-20260913T165538897145Z` | 22 passed | `7bc776a3e5ce0a7d1ecf3320ec01228d6cfc64f56073ecda3cc429a3053feba6` |
| `workspace-change-focused-integration-workspace-change-green-20260913T165544853531Z` | 2 passed, 1 ignored | `2af6de1bb5bac169bdbffe7cb2ce3d40829b6da6810736b255ab078b48184324` |
| `workspace-change-installed-owner-control-history-final-20260913T165558948767Z` | 1 passed, both pinned external owners | `a9a48f2ee0f22d2b6e8448bbb12885805663a791050e8392a1732895471288db` |
| `workspace-change-terminal-workspace-status-screen-green-20260913T170037489063Z` | 1 workspace terminal case | `785ad7a8bc3b7e6356651c120953f3a9ca46e938f4c9e44fc0b481a6cae7cce1` |
| `workspace-change-terminal-default-baseline-green-20260913T170044090531Z` | 4 terminal adapter cases | `6556647b0130ab0b68b77763354528b7186a283a145892e8c2de47a61d83439a` |

The terminal assertion now reconstructs the screen and checks the explicit
Workspace status field; typed command echo cannot satisfy it. Earlier failed
escape-stream assertions remain retained. These are focused development results,
not final specification approval, full regression or Cairn evidence. Dynamic
watch installation and DirectoryAdded remain required subsequent work.

## Independent specification review: changes required

The independent review of frozen candidate
`lifecycle-workspace-change-parent-source-freeze-20260913T171335211872Z`
returned FAIL. The full report is retained as
`/home/shawn/demoncoder-check-tmp/workspace-change-spec-review.md`.
All 2,839 candidate inputs remained unchanged during review. No review build or
test ran, and full regression and QUALITY have not started.

Three findings require implementation and failing/corrected controls:

1. Final workspace publication must revalidate recovery state, original live
   lifetime, exact funding and remaining allowance after awaited teardown.
   Pre-teardown checks do not establish final authority.
2. Generic lifetime validation lost its physical workspace check without gaining
   an exact applied-root chain. Validate the original owner, each prior occurrence,
   exact predecessor and one-step generation; preserve the separate model chain
   and original SessionEnd root.
3. The close timer only runs after Cancel/Shutdown or a closed command channel.
   An ordinary workspace request must also bound a provider that never closes,
   retaining the old root and held teardown when the bound expires.

The reviewer also identified a possible stale Git status update after a workspace
change for QUALITY assessment. Transport partial-send fault behavior remains an
unexecuted coverage limit, not a demonstrated additional defect. Existing focused
passes do not establish the newly requested violating cases. Retain this failed
candidate and report; re-review a fresh capture after repairs.

The repair's executed negative controls reproduced SPEC-1 and SPEC-3. The parent
verified these raw log hashes, exit codes and recorded `CARGO_BUILD_JOBS=8`:

| Scratch prefix | Executed result before repair | Log SHA-256 |
|---|---|---|
| `workspace-change-spec1-final-authority-behavior-red-20260913T172930342164Z` | 5 violating cases failed; unchanged-authority control passed | `3204d8282ae71367339df1fb5092275c00e975bc2b2bdb38e00606b64b16223e` |
| `workspace-change-spec1-delayed-close-expiry-red-20260913T173109648854Z` | 1 failed: allowance expired during actual close | `cfeb127850b9532edc2287d36e2e649dc564dbd80c36b1dd3a3fa2f914df16c2` |
| `workspace-change-spec3-ordinary-never-close-behavior-red-20260913T173048898659Z` | 1 failed: ordinary close remained pending | `fb5b5b2ee8b4af4638be364bb8533c7a619cb6884b30f5e6b6a68d41d9f277d0` |

The earlier `workspace-change-spec1-final-authority-red-20260913T172903164901Z`
contains no executed test result and is not counted as a behavioral negative.
Corrected evidence and independent re-review remain pending.

## Repair candidate submitted for re-review

The worker released a revised candidate with serialized final authority checks,
an exact applied-root chain, ordinary close timeout and generation-tagged Git
status. Same-path/new-inode selection has a narrow descriptor-bound exception.
Allowance resets/refreshes are rejected; legitimate monotonic settlement remains
allowed. The artificial counter-increase rejection test is retained as a rejected
premise, not a product defect.

The parent verified all 34 current files against the actual copies in
`workspace-change-repair-release-source-capture-20260913T180556134178Z`.
Manifest SHA-256:
`0330025216195257c94cd5cd0f30da383b51f08b5f9829f68259ceeadb672f59`.
The repair handoff is `workspace-change-repair-handoff.md`, SHA-256
`c7afeff47dbc0060f6e0c2d8711eb7452e8dbddafe6f16685f3274e45040d193`.
These release copies were made after focused checks, not before their execution.

Parent-verified command results and log hashes are retained in
`workspace-change-parent-spec-repair-green-audit.json`: final-authority 8 passed,
workspace 39 passed, installed Claude/Codex 1 passed, final close group 4 passed,
and actual unchanged-deadline expiry 1 passed. All recorded the 8-job cap. The
39-test run precedes the final test-only close-grace change; the affected close
group and static checks were rerun afterward. Full regression remains pending.

The earlier 38-pass/1-failure close test included post-timeout journal work in its
measurement. The revised test measures close-future drop, with separate outer
setup allowance. Production close grace stays five seconds. The final test-only
grace is two seconds to permit actual original-deadline expiry within that bound.
`workspace-change-spec1-delayed-close-actual-deadline-red-20260913T180358402727Z`
failed because the shorter test grace reached the close-timeout boundary first;
it is a coverage-control failure, not proof that production published after that
unchanged deadline. The corrected test passes without changing the deadline after
admission. The original authority-mutation negatives remain separate evidence.

Ripwire release quality-delta still exits 2 (245 rows, 71 gating) and test-gate
still exits 4 (109 targets, 608 untested symbols). These are nonpasses for review,
not waived gates. Independent specification re-review and QUALITY remain required.

## Second specification review: lineage repairs required

`workspace-change-spec-rereview-180902.md` returned FAIL against frozen candidate
`lifecycle-workspace-change-parent-source-freeze-20260913T180902242816Z`.
The reviewer accepted the final authority checks, ordinary timeout, narrow
same-path exception and stale Git-status repair. All 2,839 inputs stayed unchanged;
the reviewer ran no builds or behavioral tests.

Two source-proven regressions require focused reproduction and repair:

- Applied facts must remain resolvable after a later hold or reconciliation.
  The current chain rejects those records, so a valid applied B cannot be read
  through the normal root accessor after observer/installation failure or uncertain
  handoff. Keep B inspectable while separate admission rules continue to hold work.
- New transitions select the last attempted workspace operation as predecessor,
  while validation requires the last Applied operation of that lifetime. A failed
  attempt followed by explicit reconciliation and a fresh selection can therefore
  apply a root whose history the accessor rejects. Use the same lifetime-scoped
  Applied chain for construction and validation before durable publication.

The requested controls include actual root access after held application,
restoration/reconciliation without replay, failed-attempt retry through the real
reconciliation control, exact predecessor/generation and retained original funding.
These are requested reproductions, not tests already executed by the reviewer.

The worker reproduced all four selected lineage failures before repair: Cwd-held
root access, actual Manager post-application failure, failed-attempt predecessor,
and a repinned invalid pending predecessor that published. Their pre-run source
archive is `workspace-change-lineage-red-source-capture-20260913T182156867662Z.tar.gz`,
SHA-256 `cafbbc8dd58de166171f01d3acff9999d4397df6528dbcfb1d894a9351c55fb8`.

The repair uses one `AppliedRootChain` result for root resolution, construction and
pending-operation validation. Applied facts retain their identity after holds and
reconciliation; execution permission remains separate. The new-lifetime control
also retains prior history while selecting its own applied predecessor.

The parent verified 328 source copies against current files and the release archive
`workspace-change-lineage-release-source-capture-20260913T182758957645Z.tar.gz`,
SHA-256 `25fbc1c6cb27960dfc10d1ad43e7d450a48ecd35d337099116aa8c64c74932ef`.
This release capture follows the checks. The handoff is
`workspace-change-lineage-repair-handoff.md`, SHA-256
`bd6a22eb6f7a730d5bdd5abfa00aa04088854556862ebea4b0bb7cb493e56ece`.

Parent-verified logs in `workspace-change-parent-lineage-green-audit.json` show
44 workspace tests, one installed Claude/Codex case and one actual Manager failure
case passed, each with the 8-job cap. Targeted lifetime/model/native integration
and Manager barrier checks also passed, followed by formatting, all-target check
and Clippy. Full regression and independent review remain pending. Static tools
still report quality-delta exit 2 (262 rows/71 gating) and test-gate exit 4
(109 targets/613 untested symbols); those results remain nonpasses.


## Frozen 183142 QUALITY findings and full regression failure

Independent QUALITY report `/home/shawn/demoncoder-check-tmp/workspace-change-quality-review-183142.md` (sha256 `bc6382281a49e2f1b813e805a92e9dad1df4c0e2e45f1c760f63691730796ee8`) is FAIL. Q1: two workspace selections without an ordinary prompt leave an obsolete pending handoff deliverable on a subsequent prompt. Q2: live Creator resolution checks startup A before substituting admitted B, so removing A blocks new-root work. Both are source findings pending behavioral repair. Minor Q3 records repeated retained-handoff hashing; no measured latency failure is claimed.

The unchanged frozen candidate full Rust run `/home/shawn/demoncoder-check-tmp/lifecycle-workspace-change-parent-all-targets-final-20260913T183656381709Z` exited 101: (1163, 2, 37) (passed, failed, ignored), 49 suites. Failed tests: `session_allowance::compaction_trigger_matching_prompt_and_agent_keep_unfunded_and_exhausted_holds` and `external_correction::correction_deadline_bounds_delayed_response_dispatch`. The former now reports Agent PostCompact auto/exhausted failing in PreCompact after 27.19 seconds, with no summary; the latter timed out during a three-second uncertain-delivery retry. Causes remain under investigation; neither is declared unrelated or repaired. Both exact executables are retained under `/home/shawn/demoncoder-check-tmp/workspace-change-post-tool-failure-20260913T184646819616Z`. Pipeline stopped before subsequent checks. Freeze verification after the run passed all 2,839 inputs with unchanged HEAD and path set. This run is not completion evidence.

Both preserved failing executables passed their exact individual reruns without a rebuild: compaction 1 passed in 23.56 seconds (`lifecycle-workspace-change-parent-exact-failed-compaction-20260913T185026096171Z`), delayed response 1 passed in 31.20 seconds (`lifecycle-workspace-change-parent-exact-failed-post-tool-20260913T185101219794Z`). This does not establish a cause or a fix. Diagnostic-only assertions now retain the compaction operations and identify the delayed-response adapter/mode plus prior record; required behavior and timeouts are unchanged. These edits still require compilation and verification. The full run failure history remains open for independent review.

Q2 behavioral RED executed one failing case after startup A was renamed: `workspace-change-quality-q2-startup-root-behavior-red-20260913T185650398309Z`; assignment failed before the B provider/write. Q1 first native API attempt (`workspace-change-quality-q1-obsolete-handoff-behavior-red-20260913T185413684691Z`) was invalid coverage because native checkpoint reuse has no external handoff; it is not a product-defect demonstration. The installed Q1 RED `workspace-change-quality-q1-installed-obsolete-handoff-red-20260913T185734218404Z` then failed in Claude: the second D input retained obsolete B-to-C handoff data after consecutive selections. Log sha256 `ac817f4ee2c862fb48294efec346824821bb9ee5cf11b2957ea0930cf0486c6a`; raw fixture `workspace-change-claude-Zh94n0`. This RED stopped before Codex. Corrected proof on both installed backends remains required. Pre-run source archive `workspace-change-quality-q1-installed-red-source-capture-20260913T185715150779Z.tar.gz`, sha256 `fd4c33e6fe2a8ad581cf3d375e03791d316b1062a3685a3e70e953e01629c8ec`.

The corrected installed final attempt `workspace-change-quality-q1-installed-obsolete-handoff-final-20260913T190754864747Z` exposed an invalid test expectation: Claude may include the already delivered C-to-D frame once in subsequent same-session conversation history. An assertion demanding zero markers on the second request conflated retained backend history with new host injection. The obsolete B-to-C frame was absent in that attempt. Preserve the failed run and fixture `workspace-change-claude-ksMtxC`; corrected checks must reject the obsolete frame and duplicate current frame, and prove the original delivery receipt/provider does not change on the second prompt. This attempt is not a production-regression claim.


## Q1/Q2 repaired candidate and parent audit

Worker handoff `/home/shawn/demoncoder-check-tmp/workspace-change-quality-repair-handoff.md`, sha256 `8339e2c9f5cc9456d02b91ac803344da0910f944b729bd0daf841bb4f1b6423f`, records the exact final prefixes and limits. Parent verified all 330 current/captured source files and the archive hash: `workspace-change-quality-final-source-capture-20260913T191751348152Z.tar.gz`, sha256 `91a89f3fdbef0c74613294b294104744ee232217657f83647aff92c89629e45b`. This is a post-check capture, not proof of a pre-test source freeze.

Q1 repair atomically supersedes older pending handoffs on successful application and restricts reserve/bind/finish to the current root occurrence. Failed/new no-op transitions preserve the existing pending delivery; older receipts deserialize without a superseded field. Final installed both-owner test passed once with exact first/second prompt and stable provider receipt checks (`workspace-change-quality-q1-installed-provider-receipt-final-20260913T191145374534Z`, log sha256 `9cbcd31e61d7a7ae020ab7d25c09a55ce2b207a0557bd1b4eab50fdfa4f76aa4`). Q2 resolves live Creator selection from admitted B from the outset; actual missing-A/B-write and access/Oracle regression each passed.

Parent checked raw logs for runtime workspace 32, workflow workspace 10, lifetime 4, model-switch 9, default workspace integration 2 (1 installed ignored), and the two diagnostic named cases 1 each. Fmt, all-target check and all-target Clippy exited 0. Audit JSON: `workspace-change-parent-quality-repair-green-audit.json`. Its zero-count command records are explicitly not behavioral evidence: both `quality-q1-failed-newer-preserves-current-green-20260913T190312340953Z` and `quality-diagnostic-compaction-regression-20260913T191420722449Z` selected no tests; their later executed commands supply the actual one-case results.

Ripwire remains nonpassing: quality-delta exit 2 (273 rows, 74 gating) and test-gate exit 4 (107 named tests, 618 unmapped impacted symbols). Q3 repeated hashing remains a documented nonblocking concern, without a measured latency failure or a speculative refactor. Independent renewed SPEC/QUALITY and a new full regression are required; the two earlier full-run failures remain unexplained despite exact individual and recompiled focused passes. No implementation commit or merge has occurred.


## Renewed SPEC 192208: remaining cross-lifetime handoff blocker

Independent report `/home/shawn/demoncoder-check-tmp/workspace-change-spec-rereview-192208.md`, sha256 `5f1c2bdea95a3ea552bff1c806e986f7e8e155bcc1640d892857cff157e1afbe`, is FAIL. Same-lifetime Q1 and Q2 repairs are accepted within their tested scope, and prior five SPEC repairs remain present. SPEC-6: L1 A-to-X-to-B leaves pending B generation2; after real end/resume, L2 B-to-X-to-B produces the identical root tuple. After the first L2 prompt delivers its own handoff, the second can select L1 because reserve/bind/finish match the root but omit native lifetime identity. This is source-proven, not an executed reviewer reproduction. All 2,839 frozen inputs remained unchanged. No full regression started for this candidate.

Repair must require the exact current native lifetime together with current applied root at reserve, bind and finish; keep L1 history truthful without manufactured delivery. Add actual end/resume and two-input controls before repair, retaining same-lifetime, failed/no-op, compatibility and restart uncertainty checks. Parent read and recorded this review before releasing the next implementation action.

SPEC-6 actual installed RED `workspace-change-spec6-installed-lifetime-collision-behavior-red-20260913T193244643181Z` executed 0 passed / 1 failed in Claude before reaching Codex: the second L2 provider request received L1 pending B/gen2 handoff. Parent verified exit 101 and log sha256 `9917ce8c04182952a16638fcc4d34b730b9aafc2c4ac33d1c089f42b279ca0ec`. Fixture `workspace-lifetime-collision-claude-23oe9I`; source/test pre-run capture `workspace-change-prerun-source-capture-20260913T193151401101Z/manifest.json`. Corrected both-owner proof remains required. The new exact installed test is registered in the parent final checklist: `installed_external_resume_never_delivers_a_matching_root_handoff_from_prior_lifetime`.


## SPEC-6 corrected candidate and parent audit

The shared handoff boundary now requires the latest native lifetime and exact current root from one record snapshot at reserve, bind and finish. Prior-lifetime Pending records remain undelivered. Worker handoff `/home/shawn/demoncoder-check-tmp/workspace-change-spec6-repair-handoff.md`, sha256 `845bf4d8f4ba7823d9f571c697f71b48e79d5b4202f020fecc13be9ccd7af601`, records the commands and limits. Parent verified that only the runtime workspace-change module and installed workspace test changed among the prior 330-file capture. Both current files match their retained copies. Final archive sha256 `5c28b70175b1a1108b7342eab31d0d6f63eca898ca1f5acabacb09ba8118080e`. Its manifest accidentally lists its own earlier content hash; that self-entry does not match and is not validation evidence. All other entries match, and the actual final manifest hash is `33c0b2f450338708e835041bf766e30183012801599a2d50d5517a40d55d7211`. The artifact is preserved unchanged with this limitation.

Parent audited the actual installed RED and direct stale-bind RED, then their corrected one-test passes. The final close/resume test passed on both pins (`workspace-change-spec6-installed-lifetime-collision-green-20260913T193721236067Z`, log sha256 `8796a31b6a713aee0a979f358a85bc0dcbf0ad9acd1ecea084c0057503ebe09c`); all 120 retained fixture files match their manifest. The prior same-lifetime installed case also passed both pins. Runtime workspace 33, native lifetime 4, model-switch 9 and default workspace integration 2 passed (2 installed tests ignored in the default run). Fmt, all-target check and Clippy passed. Details and verified log hashes: `workspace-change-parent-spec6-audit.json`. The new installed test and both-owner loop/input/receipt assertions were inspected and added to the final parent checklist.

A concurrent workflow-family run failed 3 of 10 cases: ordinary never-close setup exceeded its outer bound, in-bound close did not settle within the outer timeout, and cancellation/shutdown exceeded its close/setup allowance. The unchanged serial run passed all 10. Preserve `workspace-change-spec6-workflow-workspace-family-20260913T193837549376Z` (log sha256 `9abec21f749047142879ab1bf2323a33f211d3fdfcdfc11636882070288e3b2a`) and the serial result; this contrast does not establish a cause or fix. Production deadlines and assertions were not relaxed. The earlier full-run failures also remain unexplained.

The initial installed fixture read-before-file-exists failure and ambiguous unqualified Ripwire query are not passing checks or behavioral defect demonstrations. Ripwire quality remains exit 2, with 74 gating findings; test mapping remains exit 4, with 620 unmapped impacted symbols. Renewed independent review and the full qualification remain required.


## Frozen 194731 qualification: terminal usage visibility regression

Independent SPEC passed (`workspace-change-spec-rereview-194731.md`, sha256 `2f90191fa2f160dde1c28fa9f17f21b7f5ebe4885bb8c89df2eb7fee24def2ed`) and source QUALITY passed (`/home/shawn/demoncoder-check-tmp/workspace-change-quality-review-194731.md`, sha256 `384c24f6a22b115637341a41dc6f8d7bc8a4fb83632dfefcdfd143fd672696f2`), both scoped and retaining historical failures. One reviewer freeze check saw transient path-list instability with unchanged recorded bytes/HEAD; its repeats passed. A later 100-sample parent observation found no differing paths, which does not identify that transient event.

Full Rust passed 1,167 tests, 0 failed, 38 ignored across 49 suites at `lifecycle-workspace-change-parent-all-targets-final-20260913T195445323690Z`. Clippy/fmt/diff passed. Installed Settings owners 15, both workspace installed tests 2, batch cases 2, external baseline flow checks, and terminal provider settings 13 / role settings 6 / live settings 6 completed successfully before the pipeline stopped. All five previously failing Rust cases passed in this run; their causes remain unexplained and their failed logs remain preserved.

The retained application `lifecycle-workspace-change-parent-source-freeze-20260913T194731154473Z-demoncoder` has sha256 `7623ac326e5d4c9e208b4d1c2ec5b0b0e6859e5f44841c6d2ae7e6a727dc0d0d`. Output-limit terminal suite `lifecycle-workspace-change-parent-output-limits-retained-final-20260913T201135811500Z` failed three assertions across two test methods: the success/truncation content is rendered, but expected usage text `out 12000` is missing from the visible 160-column status row. The new full workspace prefix pushes later usage fields beyond the visible row. This is a concrete visibility regression; preserve the existing terminal width and output/usage assertions while repairing layout. Full output and exact app are retained. Workspace/default terminal checks and the final pipeline freeze step did not run. A separate parent freeze verification after the stop passed all 2,839 inputs. The source freeze is now intentionally released to record and repair this finding. The prior scoped review passes do not establish readiness to commit or merge this candidate.


## Terminal visibility repair and parent audit

The footer now allocates a separate usage row only after current-turn usage is reported. The existing workspace/model/context/Git/agent row stays above it; layout supplies the resulting body and prompt geometry. TurnStarted clears and hides usage as before. Only src/terminal.rs changed from the 194731 source candidate; tests/output_limits.py is unchanged.

Actual 160-column behavioral RED is workspace-change-terminal-layout-160-visible-usage-behavior-red-20260913T201731489878Z (0 passed, 1 failed; log sha256 1c86d91964dbc2983818a84227110823a3e1f8ce6ccc4b1dbd42951cf11eb7c1). The earlier shorter awaiting-state probe passed and is invalid defect coverage, not a RED. The corrected test includes the actual zero-agent summary and passes after repair. Unchanged output_limits.py passes all three test methods; saved-limit/override and truncation explicitly use 160 columns, while large-file/tool-continuation uses the default 100 columns (workspace-change-terminal-layout-output-limits-160-green-20260913T201939579272Z; log sha256 2e3206ffb474255eac255f6d59be91527091cbbc0ae127b8fbb287aa91154b16). Workspace PTY 1, default adapters 4, terminal unit 9, status unit 3, live Settings 6, scrollback 2 and terminal sweep 4 pass; formatting, all-target check and Clippy pass. Parent checked raw results in workspace-change-parent-terminal-repair-audit.json. All Cargo commands record 8 jobs and were serial.

Parent inspected the isolated repair patch and verified both captured source files against current files. Final archive workspace-change-terminal-layout-final-source-20260913T202308413145783Z.tar.gz has sha256 f6e142de0c2d086d51408cd42f131bbf70f5bee955b850d26495996903e90160. The capture again includes an invalid manifest self-hash; all other rows match. Actual final manifest sha256 is 4ddf6022125f876636cc939b6b43801e661abb09f95f839705a556499e909df7. Preserve this artifact and limitation; the independent parent source freeze supplies separate validation. Ripwire quality-delta and test-gate remain static nonpasses, not behavioral checks. Renewed independent review and full qualification remain required; earlier unexplained failures remain recorded.


## Frozen 202514 qualification: aggregate HTTP timing failure

Independent SPEC202514 and QUALITY202514 passed the bounded source review. Full Rust lifecycle-workspace-change-parent-all-targets-final-20260913T203009799744Z completed with 1167 passed, 1 failed, 38 ignored across 49 suites; log sha256 6f5ed7d724959a4030a45b02de9fc8024927a6d6e7f2ad1a0c4358fbd352016b. The only failure was one_deadline_bounds_slow_headers_and_continuous_body at tests/plugin_http_runners.rs595: total elapsed was not below 350 ms. No actual elapsed/mode was logged. Other previously failing Rust cases passed; their causes remain unexplained. Pipeline stopped before Clippy and installed/terminal checks. After-stop freeze verification passed all 2,839 inputs.

Exact failing executable was preserved before rebuilding under workspace-change-http-deadline-failure-20260913T203445907176Z, sha256 4c587630aed702d0699d80fc6e391d2e13eb7805707d9ffb860dadd270f65e10. Its unchanged exact individual rerun passed 1 test in 1.23 s (lifecycle-workspace-change-parent-exact-failed-http-deadline-20260913T204301850021Z). Passing a rerun does not establish a cause or fix. The test times fixture creation, registration, full workflow execution and durable bookkeeping together with HTTP. Configured HTTP timeout is 120 ms; the 350 ms aggregate cutoff is a test choice rather than a product requirement. The developer questioned that cutoff. Read-only investigation workspace-change-http-deadline-investigation.md records actual timeout paths, the possible added no-handoff persistence cost and their limits. No production deadline escape is established, and no environmental cause is asserted. Preserve the original failure while measuring phases and constructing a direct deadline proof; do not merely enlarge 350 ms.

The source freeze is intentionally released for this diagnostic/verification repair. The terminal evidence wording above is now precise: original three failures came from two test methods at 160 columns, and the third method uses 100 columns and did not fail.


The diagnostic retained the original 120 ms timeout and 350 ms aggregate assertion, and failed in the delayed-header mode: total 883.284418 ms, fixture creation 5.444786 ms, registration 22.128757 ms, executor construction 690.699506 ms, actual turn 164.961201 ms and final record read 39.963 microseconds. The recorded reason was HTTP hook timed out; remote effects may be unknown, with one request and no write. Log workspace-change-http-deadline-diagnostic-original350-20260913T204445864281Z has sha256 98d1a03f20ee429f103b268cd235a3432a3ce81a9d108e36c9cff7faf15ce88c. This directly demonstrates the aggregate assertion can fail due to unrelated setup; it does not identify the original full-run cause, whose phase timings were absent.

Independent verification-plan review workspace-change-http-verification-plan-review.md accepts dependency-based reuse for a test-only repair. Parent captured compiler dep-info for all 49 original targets in workspace-change-202514-target-dependencies.json; only the HTTP integration target includes this test root. Final qualification must compare frozen production/shared/build inputs, rerun the entire HTTP target with normal concurrency, then finish the pending installed/terminal pipeline alongside QUALITY. It will be reported as composite evidence, never as a retroactive pass for the failed full command. Any changed production/shared dependency invalidates this reuse and requires reassessment.


The repaired test prepares the fixture and executor before the observed request, then keeps the local server unfinished until a test-owned release. It requires one request, no retry or tool write, retained uncertainty and an allowed stage-specific failure while the server has not completed. The existing generic request/body transport-failure strings do not independently identify a timeout; controlled peer behavior and the deliberate disabled-deadline controls supply that distinction. The separate two-second guard prevents a hung test; it is not a replacement product deadline. Configured HTTP timeout remains 120 ms.

Both total-deadline controls failed as required: delayed headers workspace-change-http-deadline-total-bound-violating-control-20260913T204652223399Z (log sha256 b8a4b47ad7265a19e7135859396ccad655162238638088e12caaa37ddd142d2a), and streaming body workspace-change-http-deadline-stream-total-bound-violating-control-20260913T205028243089Z (log sha256 f573bdbdbc1f1ec5e2dba6c78579642e03dfbe3e7498cf01285405662eef8c99). Temporary production mutations were restored. Exact corrected test passed both modes (cd47ebfc7a7a1b32924ae4f8a075f435ad3424846c93f12cb8665b140b41d42d); complete HTTP target passed 32 tests (499ad5d12bec115cb99c1170bce27236c20e0bcc3c85fc663f310f257d828d14). Parent compared frozen production inputs after restoration: only the HTTP test and this review differ from 202514. Final post-control checks, source capture and independent review remain required.

Intermediate fixture attempts are retained rather than treated as passing evidence: workspace-change-http-deadline-total-bound-corrected-20260913T204718772127Z lacks a completed test-result line after an assertion/cleanup issue, and workspace-change-http-deadline-total-bound-green-20260913T204906567667Z actually failed because the allowed response-stage category was incomplete. Corrected cleanup ordering and stage-specific assertions are test-harness repairs, not claims of new production fixes.


## HTTP-SPEC-1: configured deadline still needs direct proof

Independent SPEC205255 failed the test-only candidate (workspace-change-spec-rereview-205255.md, sha256 919c2a8fd5a189b684c8cd60ba973e53e938d5a192cf34154faf1a473472b508). Its two-second guard rejects absent total deadlines but permits a one-second timeout in place of configured 120 ms. Request-to-receipt timings were logged, not asserted. This is a source-proven verification gap, not an executed production defect. The 32 passing tests and both disabled-deadline controls remain valid within their limits; composite qualification did not start.

The next repair must observe client disconnect from the peer independently of receipt persistence, apply a documented configured-deadline plus scheduling/poll tolerance, and fail actual one-second deadline mutations independently for headers and body. Retain deterministic cleanup and all no-retry/write/uncertainty checks. All 2,839 frozen inputs remained unchanged through review. The freeze is now released for this bounded test repair; production changes remain temporary controls only and must be restored exactly.


## HTTP-SPEC-1 direct runner completion proof

The suggested EOF observation did not work on the unchanged 120 ms path: the operation settled but the peer did not observe EOF within its guard. Retain normal-disconnect-probe-20260913T210114067515Z and both earlier EOF mutation attempts as invalid measurement coverage, not deadline defect demonstrations. No cause for that socket observation is asserted.

The test instead wraps the registered real HookRunner in a test-only delegating TimedRunner. It records completion immediately after the real HTTP runner returns, before hook receipt persistence and native settlement. The peer independently records request acceptance. The asserted interval is configured 120 ms plus an explicit 130 ms scheduling tolerance; the separate two-second guard remains only test cleanup protection. This is a measured test allowance, not a new production timeout. The source remains unchanged outside the HTTP test.

Actual finite one-second mutations of both total deadlines fail this direct assertion: headers measured 998.926207 ms (workspace-change-http-spec1-runner-one-second-headers-red-20260913T210239422961Z, log sha256 5248c24cdac0b0530633b0576de5419eb9cf5020c70da6cd4b54f9f1193bf45b); streaming measured 999.480465 ms (workspace-change-http-spec1-runner-one-second-body-red-20260913T210304081887Z, log sha256 569816fd2a07c6d984e0e88d4d2d48fcd3f2618d24a9cdbf530b675869491e7c). Both fail before the two-second guard. Restored normal cases measured 120.401186/118.161082 ms and passed. The complete HTTP target then passed all 32 with normal concurrency; its direct intervals were 120.784533/120.143675 ms (workspace-change-http-spec1-http32-final-20260913T210359781828Z, log sha256 4c1c17d1b99931633859a4eb6448d30ab79648ec659e843ee5024c88946ff73a). Parent raw audit: workspace-change-parent-http-spec1-audit.json. One request, no retry/write, uncertainty and controlled peer release remain asserted. Final capture and fresh independent review remain required.


## Final bounded qualification and self-audit

SPEC210511 passed (report sha256 ff8e86fce42ffe9a674bbe72ed74d54a34f4da68ae2542aded31f7b9e66f5f87); QUALITY210511 passed (report sha256 988f84bb4d21e695e81e29d15275dc5822ea8d9a33c8e391f7f4a4a215a11434). Parent read both reports and verified the final four-row source manifest and captured source copies. Both reviews independently checked the test-only delta and composite reuse boundary. Final qualification checkpoint workspace-change-parent-composite-checks-20260913T210841654785Z.json has sha256 fbcae869b7f7d35654c150d5946d71b21e5177a5224dedb553d3c806deacd44d; all 21 command stages completed successfully. Parent audit workspace-change-parent-composite-final-audit.json has sha256 f93e35f36a0d91eb9675a44c37f9d094d4b8a3f96daf28e579f4916c496c4e44.

This is composite development qualification: 1,136 passing tests and 38 ignored across 48 unchanged Rust targets from frozen 202514, plus all 32 HTTP tests rerun successfully on frozen 210511, yielding 1,168 passing core tests. The original full 202514 command remains failed; no historical receipt was changed. Clippy, formatting and diff checks passed. Installed Settings ownership passed 15 cases; both installed workspace tests passed on Claude 2.1.267 and Codex 0.153.4; batch cases passed 2; external Submit/Stop baselines passed 8. These use controlled provider peers, not fresh paid/live-provider qualification.

The exact retained application has sha256 daa52c9dd2aad59df493ee59e94aec38edc435be9420a41d51dbae0c953010c7. It passed terminal provider Settings 13, role Settings 6, live Settings 6, output limits 3, workspace transition 1 and default adapters 4. Output-limit saved/override and truncation methods use 160 columns; the large-file method uses 100 columns. Original terminal failures were three assertions across two methods. Final before/after freeze checks matched all 2,839 inputs, unchanged HEAD and external dependencies. Every build/test wrapper retained the eight-job cap, with one Cargo owner.

Parent self-audit: the bounded workspace replacement and required regression repairs follow the production rules, with no remaining Critical/Important finding in this scope. Reviews attacked final authority, lineage, current-root/lifetime handoff delivery, missing-startup-root Creator resolution, Git staleness, terminal visibility and configured HTTP deadline proof. Corrected cases and actual negative controls are retained. Earlier unexplained failures, invalid coverage attempts and static Ripwire nonpasses remain disclosed; Q3 repeated hashing remains Minor without a measured latency failure. This does not establish full lifecycle dispatch or the 41-requirement commitment: dynamic watches, DirectoryAdded and complete package/live conformance remain required. Final metadata below the tested source may now be committed; no runtime/test code changed after qualification.
