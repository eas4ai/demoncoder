# External Submit and Stop integration

Status: Claude/shared prerequisite verified; Codex integration and full commitment remain pending.

The final unchanged candidate passed 800 Rust tests (17 ignored, 44 suites) and
22 actual pinned-Claude cases against a local model peer. Fresh independent
specification and quality reviews passed. Strict clippy, formatting, Python lint
and diff checks passed. Ripwire quality/test-gate returned diagnostics (exit 2/4),
reviewed without suppression; they are not passing checks.

The adapter now delivers actual Submit/Stop callbacks through the active Backend
owner and durable Pending/Sent/Acknowledged receipts. It preserves the original
task allowance, distinguishes plugin correction origin, holds uncertain delivery,
and retains exact post-tool replay authority and completed effects. Controlled
child ownership, translation and recovery tests supplement the actual root matrix;
actual child/live-provider/full-package qualification remains later work.

The large adapter loop and bounded 1,024-callback session history remain explicit
maintainability/availability limits. No blocking finding remains in this bounded
prerequisite. These are development checks, not Cairn evidence receipts.

The following development record preserves the failures and intermediate status
at each checkpoint. The final candidate and review results below supersede earlier
pending statements.

This stage implements the Claude/shared portion of the [external callback decision](../decisions/authorize-external-non-tool-callbacks-through-their-actual-lifecycle-owner.md).
Actual source callbacks must use the active backend operation, original allowance
and durable hook receipts. Source run identity and host occurrence identity remain
distinct. The native lifecycle stages and source-only qualifications do not prove
this integration.

## Baseline failure demonstrations

The new `tests/plugin_external_non_tool.py` launches the Rust production-adapter
fixture against pinned actual executables and local model peers. It does not
install hooks or manufacture callbacks. Credentials are disposable fixture values;
these checks are controlled transport evidence, not live-provider evidence.

Retained results under `/home/shawn/demoncoder-check-tmp/`:

- `external-non-tool-claude-red-2`: Claude completed one actual model request;
  the host reported completion, but zero lifecycle callbacks were recorded where
  the Rust fixture required two. The test failed as intended.
- `external-non-tool-codex-red-1`: Codex likewise completed one model request,
  with no peer errors and zero callbacks against the required two. The test failed.
- `external-non-tool-claude-red-1`: authentication failed before model execution.
  This is a launcher diagnostic, not a meaningful integration failure. Replacing
  its synthetic API key with a synthetic OAuth token exercised the existing
  subscription check without changing production authentication.

Each run retains the executable and launcher hashes, model requests, host result,
host events, runtime receipt and Rust assertion output. No successful integration
or complete conformance is claimed yet.

The first implementation diagnostic, `external-non-tool-claude-green-1`, failed
despite its directory name: the adapter lacked the source session observation
needed before Submit. The source qualification enabled `--replay-user-messages`;
the adapter previously enabled it only for post-tool plans. The repair enables
that source observation for ordinary plans and preserves strict session matching.
Its reruns still lacked a session observation. Diagnostic byte capture showed
that production omitted the outbound user-message UUID supplied by the source
qualification. With that UUID, `external-non-tool-claude-green-5` passed strict
command-start/session correlation, then failed at the original backend owner
check. Root backend records use their existing implicit record identity; child
records require explicit identity. The repair preserves that distinction.

The optional `--trace-wire` launcher mode introduces a diagnostic wrapper and
bounded byte tee. It is not the final direct-execution topology. Arbitrary-byte
round-trip and over-limit rejection controls passed; normal qualification must
run with the option absent.

Static review identified a silent BrokenPipe catch in that diagnostic helper.
It now reports capture failure, kills and reaps its child, and exits nonzero.
The byte round-trip, size-limit and closed-input controls all passed after repair.

## Preliminary actual Claude results

Four direct-execution cases now pass: `external-non-tool-claude-green-7`
(one model request, two receipts), `external-non-tool-claude-submit-deny-1`
(zero model requests, one receipt), `external-non-tool-claude-stop-correct-1`
(two requests, three receipts), and `external-non-tool-claude-always-block-1`
(two requests, three receipts, unmet gate). The correction cases retain one
Submit and two Stop occurrences under the original backend operation. The peer
independently verifies that Stop feedback appears in the second request only.
Failure, cancellation, recovery, cross-dialect and regression checks remain
pending; these results do not freeze or approve the implementation.

The first six direct-execution failure/control runs also exited zero, with actual
model request counts: cancel-submit 0, shutdown-submit 0, cancel-stop 1,
malformed-submit 0, malformed-stop 1, timeout-submit 0. Their directories use
`external-non-tool-claude-<case>-1`. The timeout exercised the real 30-second
deadline. However, `host-result.json` mapped every successful Rust return to
the text `complete`, obscuring cancellation and shutdown outcomes. The fixture
must retain exact outcomes in its final reruns. These preliminary reports must
not be cited as successful completion after cancellation.

Two later protocol controls failed meaningfully: an unrelated command UUID
acknowledged callback delivery, and a forged Stop assistant text was accepted.
Those are open implementation findings until the tightened correlation and
pending-delivery admission checks pass their reruns. Earlier successful actual
cases do not establish correctness against these adversarial inputs.

Actual source exploration exposed a text-correlation detail: a final whitespace
block can be absent from Claude's emitted assistant records while still causing
Stop to omit `last_assistant_message`. Correlation therefore follows the ordered
text stream through message completion, including blank blocks. It cannot fall
back to the previous nonempty assistant record or concatenate the entire turn.

Eight direct production text cases passed in
`external-non-tool-claude-text-<case>-1`: plain, multiple, trimmed, blank-final,
empty-middle, zero-width, bom and next-line. Each made one model request and
independently checked the retained source Stop text. The source trims U+FEFF
but preserves U+0085 and U+200B; using Rust's generic whitespace trimming would
change that behavior. Final candidate verification remains pending.

## Source correlation and registration findings

In pinned Codex, `core/src/hook_runtime.rs` emits and awaits the started event
before polling Submit execution or calling Stop execution. Completion follows
execution. Independent stdout and relay readers can still observe arrivals in a
different order; correlation must accommodate bounded scheduling delay under the
same active owner. Source run IDs repeat across Stop rounds, so they cannot alone
identify an occurrence.

Codex hook discovery appends configuration layers and plugin declarations.
SessionFlags overrides can disable ordinary hooks, but managed and built-in
hooks ignore that disabled state. Listing hooks before thread creation does not
freeze later discovery. Private registration and ambient-hook isolation therefore
remain an explicit implementation question; enabling the hook feature alone is
insufficient. The managed compaction registry itself forces ordinary hooks off
and preserves that suppression through refresh. A read-only assessment found no
safe configuration-only registration route. The [separate managed ordinary
boundary decision](../decisions/freeze-private-ordinary-codex-hooks-beside-the-managed-compaction-boundary.md)
records the required extension. Existing compaction declarations and
acknowledgments keep their separate contract. Normal source parser errors can
also mark a hook failed without stopping model work; immutable registration
alone does not establish the required failure behavior.

## Mixed correction interaction — in progress

The worker reported a broad Rust run with 794 passing tests and 17 ignored
across 44 suites. A subsequent library run passed 297 tests and clippy passed
with warnings denied. These precede the mixed correction repair and do not
approve the final candidate. The protocol correlation controls above were
repaired and their focused reruns passed; final frozen verification is pending.

The actual Claude mixed baseline `external-non-tool-claude-mixed-red-1`
failed meaningfully: one model request, one ordinary callback rather than three,
and an exact completed `proof.txt` effect containing `external mixed effect\n`.
The peer recorded no errors. The host rejected the correction with
`source lifecycle command identity differs`. This is a handoff failure after a
real completed tool, not a model-peer failure. The recorded decision
`bind-source-submit-during-post-tool-correction-to-the-admitted-plugin-handoff`
requires a narrowly bound plugin-origin Submit while preserving post-tool replay
acknowledgment and the original correction allowance.

Actual source probes show the correction Submit arrives after its own command
started event and before replay acknowledgment. Prompt IDs change for this new
source command. Array content contributes only text blocks, joined by newlines
and trimmed as a whole; image content remains part of the frozen command but is
absent from the source Submit prompt. A separate plain-string probe preserved
leading and trailing spaces unchanged. These exploratory observations are in
`claude-mixed-post-ordinary-content-1` and `claude-submit-trim-probe-1`; they are
not substitutes for production adapter qualification.

A two-submission source probe (`claude-two-submit-probe-1`) also observed the
previous command's `completed` notice after its result and after the host sent
the next user message, before the next command's queued/started notices. Both
ordinary submissions completed with two model requests and no peer errors.
The host must recognize only the exact retired command's terminal notice; it
must not ignore arbitrary mismatched command identities. A production
`pass-twice` case is being added to check session reuse across this ordering.

The production `pass-twice` baseline subsequently failed meaningfully in
`external-non-tool-claude-pass-twice-red-1`: one model request, no peer errors,
and `source lifecycle command identity differs` on the second turn. The test
requires four actual ordinary receipts across two backend operations with the
original task and allowance. This remains open until its repaired rerun passes.

The four repaired production cases passed in
`external-non-tool-claude-{mixed-green,mixed-deny,mixed-cancel,pass-twice-green}-1`.
The correction pass made two model requests and completed. Denial and cancellation
made one request each; denial retained an unmet gate and cancellation retained
`cancelled`. Each mixed case retained the exact original write effect. Correction
Submit (and final Stop for the pass) retained explicit `plugin_post_correction`
origin, the post operation, and its frozen content digest. Ordinary session reuse
completed with two requests and four actual callbacks. These are development
runs; final frozen matrix and independent reviews remain pending.

An actual pending-handler backend exit control passed in
`external-non-tool-claude-backend-exit-1`: zero model requests, no peer errors,
and `backend transport exited during lifecycle callback`. The wrapper records
its PID before direct exec; the Rust fixture opens a pidfd and verifies the
source/supervisor/test ancestry before killing that source process. The host
remains alive and checks the pending unsettled receipt and refusal to replay.
This is distinct from cancelling the host turn. Final matrix includes this case.

The first frozen matrix (`external-non-tool-claude-final-1`) stopped on its
fourth case, `stop-correct`: only two ordinary callbacks arrived rather than
three, despite two model requests and no peer errors. The host rejected the
second message start with `source response command identity differs`. The first
three cases passed; this is not a passing matrix or approved candidate.
A diagnostic byte tee reproduced the failure in
`external-non-tool-claude-stop-correct-trace-2`: Claude omits both user-message
UUID fields on the model response to admitted Stop feedback. It keeps the source
session, and the following Stop uses the original prompt ID with
`stop_hook_active: true`. Any allowance for omitted fields must be restricted to
that admitted Stop continuation; explicit mismatched identities remain invalid.

A focused control through actual Stop settlement and correction admission failed
before the omission repair and passed afterward (RED log
`/home/shawn/.local/share/rtk/tee/1789156519_cargo_test.log`). Direct backend
reruns `external-non-tool-claude-{stop-correct,always-block}-repair-1` then passed:
two model requests and three callbacks each, with completion for one correction
and an unmet exhausted-allowance gate for repeated blocking. The obsolete broad
Rust run was stopped with exit 130 and is not counted as passing evidence.

## Frozen candidate 2 — actual matrix passed, review pending

All 22 direct-exec Claude/local-model cases passed in
`/home/shawn/demoncoder-check-tmp/external-non-tool-claude-final-2`.
`inputs.json` freezes Rust, Python, Cargo inputs and the test executable;
`results.json` records case exits; `outcomes.json` records host terminal states,
model counts and callback counts. The runner compared every frozen input before
and after each case. All peer error lists were empty. The timeout exercised 30
seconds. Cancellation and shutdown retained their exact terminal states.

The Rust manifest is `external-non-tool-final-candidate-2.json`, SHA256
`56435aa93a46d03088a4d16aca689cb2d45c8a3fb84e079b3d8260edf0bcc71c`.
The actual fixture executable SHA256 is
`6da88e9299dfadc96793a33cb80469b873225bad903c4ce262534b7b42ba7dd3`.
Strict clippy, formatting and diff checks passed. Final broad Rust regression and
fresh specification/code-quality reviews are pending. Static Ripwire quality and
test-gate runs returned nonzero diagnostics; they are not passing checks and their
findings remain visible to review. These cases establish controlled transport
behavior, not live-provider or full package conformance.

## Final regression and independent review

`external-non-tool-all-targets-final-2.json` records `rtk cargo test --all-targets`
exit 0: 800 passed, 17 ignored, 44 suites, 499.73 seconds. RTK returned aggregate
success output, not a full raw success log; the report states that limitation.
All 16 changed Rust files and three executable hashes matched candidate 2 after
the run. The actual 22-case matrix separately matched its wider frozen input set.

Fresh SPEC and QUALITY reviewers independently matched candidate hashes and all
22 case reports. They inspected the owner, delivery, cancellation, source text,
retired command and mixed correction boundaries. QUALITY additionally checked the
three actual completed tool effects and correction request sequences. Neither
found a blocking defect or reran Cargo; their reports explicitly awaited the
broad regression that subsequently passed. Reports are retained in
`external-non-tool-spec-review.md` and `external-non-tool-quality-review.md` under
the scratch evidence root. The separate managed Codex source extension remains
unbuilt; its earlier zero-callback baseline remains a failure, not qualification.

Parent self-audit: scope, source provenance, error/cancellation handling, durable
recovery, bounded resource use, tests and documentation were checked against the
production standard. No known blocking deficiency remains in this prerequisite;
no full-commitment completion or live-provider compatibility is claimed.
