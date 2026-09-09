# Release follow-up review

commitment: release-followups
commit: 182208a95c45251564eb34e61f57781bf0ad629e
findings:
  - resolved: OUTPUT-001: Both live-driver overrides and the distinction between model quota and authentication failure are documented.
  - resolved: OUTPUT-001: The manual now agrees with discovered/default output limits and saved/CLI overrides; current output-limit checks pass.
Status: in progress

## OUTPUT-002 mechanism review

Read the current requirement and falsifier, output-limits declaration and runner,
all Rust output-limit tests, actual terminal output tests and the prior mechanism
review. The declaration includes runtime, tests, scripts and manual inputs. The
shell stops before result lines if a constituent fails.

Executed the 12 existing Rust cases: all pass. Safe truncated Anthropic text and
valid-looking tool responses for both output/context limits fail before tool
effects, retain usage, and allow a subsequent complete response without pending
tool records. The OpenAI incomplete-response case likewise preserves usage and
prevents its write. Complete responses and explicit/discovered limits pass.
Executed all three actual-terminal output tests: all pass, including visible
truncation, usage and a working next prompt. These demonstrate refusal of the
violating input and acceptance of corrected complete input without code changes.
No mismatch was found between OUTPUT-002 and its mechanism. The separate known
manual error remains an implementation action, not a mechanism edit during review.

## Live Oracle availability

The real saved Claude/fable/high Oracle check failed before producing its verdict
pair. A bounded no-tools CLI diagnostic confirmed an exhausted Fable usage limit,
while auth status confirmed the existing logged-in first-party subscription.
The observed failure is retained under .cairn/evidence/live-oracle/. No model,
authentication or billing setting was changed. The CODE-010 mechanism records the
required live verdict evidence as unverified; CODE-001 through CODE-009 pass.
This account-side limitation cannot be repaired by changing the check footprint.
The four live connection sessions have not yet been rerun in this commitment.
The manual correction and queued decision closeout remain pending.

## Connection-level clarification and Oracle test selection

The developer corrected the earlier escalation: Fable's quota response confirms
authenticated reachability and is a model-specific limit, not a connection blocker.
The prior failure record remains intact. Complete coding/Oracle cycles still have
their own execution evidence. The existing coding runner already allows --model;
the Oracle runner now accepts the same explicit per-run selection and retains the
actual model without editing saved settings or changing credentials/transport.
The old runner safely rejected --run --model sonnet before any provider request.
Empty selections and model selections without --run are explicit usage errors.
The live corrected case is checked against committed inputs below.

## Live evidence and queued socket decision review

All four real default transports completed both live coding turns. The OpenAI
API and Codex used gpt-6-astra; the Anthropic API and Claude subscription used
explicit claude-opus-5 overrides. Their original read/write/edit/Bash calls,
completed assertions and retained source passed the existing validators. The
Claude subscription Oracle used explicit sonnet for its two verdict-only cases;
it allowed the disposable outside read and denied the home-move proposal without
executing either proposal. A before/after SHA256 comparison confirmed the private
settings file remained unchanged. Model-specific quota observations remain in
history and are not represented as completed work.

Reviewed the queued Unix-socket decision and reliability-sockets runner against
REL-001. Its executed cases deny pathname/abstract Unix sockets and datagram
bypasses in confined Bash, retain permitted networking and stream socketpairs,
and verify explicit host-mode Unix sockets remain available. The consequences
for local Docker, SSH-agent and database sockets were explained to the developer.
Record the confirmed follow-up review and clear its queue entry; no runtime policy
changes are required. The manual paragraph remains the sole open finding.

The 13-line Oracle test-runner change preserves saved assignments unless an
explicit per-run model is supplied, rejects empty or non-run overrides, and keeps
transport/authentication checks intact. Formatting, compilation, targeted Clippy
and the actual live verdict pair pass. A focused baseline-pinned Ripwire comparison
reported one minor test-length increase, no gating regression; its test map does
not discover the explicitly ignored live entrypoint, whose real execution above
supplies evidence. No production runtime code changed.

## Final examination and acceptance

Reviewed the complete follow-up change: explicit model selection is limited to
the live Oracle test runner, the manual describes existing verified behavior,
and the queued socket-policy review is recorded without changing runtime policy.
All six selected requirements have current passing receipts with actual command
exit zero. Their stdout/stderr hashes were verified. All four live coding records
and the live Oracle pair pass their current-input validators. The saved settings
file still matches its pre-check SHA256. No production executable changed.

The manual now distinguishes provider output defaults, Anthropic model discovery,
positive saved limits and the invocation override. Existing tests inspect actual
native requests and reject incomplete tools. The corrected paragraph was compared
with the agreed OUTPUT contract and CLI behavior; no contradictory cap remains.

Production self-audit against rules 1–14: scope and ownership remain bounded;
credentials and saved defaults are preserved; error and quota observations remain
honest; runtime behavior is unchanged; execution evidence covers the test-helper
change; documentation, decision review and cleanup are complete. No unresolved
finding remains. The earlier account-side blocker assessment is superseded by the
developer clarification and the successful current live cycles.

## Manual clarification verification

Compared the two new passages with both live drivers’ executed --help output,
the retained successful override checks, and the recorded authenticated quota
response. Both drivers support explicit model selection; saved settings remained
unchanged during the recorded live checks. The troubleshooting entry distinguishes
reachability from completed work and states that model selection is explicit.
No runtime or test code changed. git diff --check passed.
