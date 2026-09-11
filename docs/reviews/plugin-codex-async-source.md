# Codex asynchronous command source qualification

Status: source qualification passed specification and quality review.
This is source evidence, not DemonCoder async or live-provider conformance.

The fixture runs the pinned managed Codex app-server against an isolated local TLS
model proxy, synthetic authentication and one host-served dynamic tool. It trusts
only the two fixture command hashes in a fresh private configuration. Executable
SHA-256: `80315a32acf1b625129a46b0bd75537cf76ef09701ae986154159076cc0b6aff`.

| Case | Context in model requests 1 / 2 / 3 | Observed order |
|---|---|---|
| Post-tool async, configured exit 0 | absent / absent / present | Request 2 and foreground completion precede the command end marker; context reaches the next explicit turn. |
| Synchronous control, configured exit 0 | absent / present / present | Request 2 follows command completion and already includes its context. |
| Post-tool async, configured exit 2 | absent / absent / absent | Foreground completion precedes the end marker; the next turn does not receive that context. |

Every case has one exactly correlated dynamic tool call, two explicit host turns
and three model requests. The second host turn starts at least 1.9 seconds after
the first completion and half a second after the command end marker. No extra
model request precedes that explicit turn. This is a bounded idle observation,
not proof of indefinite absence of future work. Delivered source context is a
Responses developer-message `input_text` block; unrelated metadata cannot satisfy
the verifier. Host delivery still needs attribution and cannot grant developer
control authority from that source role.

Commands record a 400ms delay and deterministic configured exit. The end marker
precedes `sys.exit`; no independent command OS exit telemetry is captured.
Original dynamic-tool response text is preserved in all three cases, including
the async-failure case. These fixtures do not exercise source shutdown or restart.

## Mechanism and evidence

`tests/plugin_codex_async_source.py` reuses the existing app-server exchange,
TLS peer and cleanup in `tests/plugin_codex_post_source.py`. The optional observer
scenario adds the delayed post command and explicit second-turn observation.
The original success, failure and correction scenarios still pass with the
modified helper; their default protocol and assertions remain active.

Three async source cases and 38 corruption controls passed. Controls challenge
raw events and model requests, source identity, command/configuration evidence,
reversed async/sync timing, an early second turn and misplaced context. Executable
and source inputs are hashed before and after execution. Output must be fresh;
optimization-disabled assertions, a wrong executable and output-directory reuse
were each refused. Ruff passed for both scripts. Black checked the new driver;
the existing shared helper keeps its established formatting.

The final run is at
`/home/shawn/demoncoder-check-tmp/codex-async source-1/qualification.json`, with log
`codex-async-source-1.log` in the parent directory. All 36 artifacts, including
six captured input files and the three session transcripts, were
independently rehashed. The original three-case regression exited zero at
`/tmp/demoncoder-codex-post-source-1ex6mc18`; its 12 recorded artifact hashes and
captured helper hash were checked. Its log is `codex-post-async-regression.log`.

Final driver SHA-256:
`8c30144c0a19933abf28c880bffc324872f8af429a2c93c424ad4929c9db62da`.
Shared helper SHA-256:
`29a3866c3fef0639dcc75c5a73ed4f6a46bdd8425c20a8753af93b8d36429b5b`.
Async fixture SHA-256:
`c4c1e29c38222dc765ef37a338cd3b2c280fd9d01a6bebf871cbf18882c5f359`.
The exact six-file input set, including unchanged peer/auth helpers and post
fixture, is retained in `plugin-codex-async-source-inputs.json` in the temporary
parent directory and in the qualification's captured inputs.

Independent specification review approved the six frozen inputs, followed by
quality approval. Both replayed all three cases and 38 corruption controls,
verified all 36 artifacts, and replayed the original three regression cases.
Specification review also rejected seven timing and misplaced-context attacks.
Reports are `plugin-codex-async-source-spec-review.md` and
`plugin-codex-async-source-quality-review.md` in the temporary parent directory.
Neither reviewer repeated the source executable run. Host lifecycle ownership,
cancellation, recovery, bounded delivery and complete installed/live evidence
remain required in this commitment.

## Production self-audit for the source fixture

| Rule | Assessment |
|---|---|
| 1. Understand | Compared actual async context with synchronous and failed-command controls. |
| 2. Coherent change | Added an optional scenario to the existing source peer and reused its correlation checks. |
| 3. Maintainability | Kept async assertions in a separate driver and preserved the original default scenarios. |
| 4. Boundaries | Bound source thread, turns, calls, prompts, transcript and model delivery to retained raw evidence. |
| 5. Errors and secrets | Used synthetic authentication and private output; failed assertions cannot write a passing qualification. |
| 6. Security | Pinned the executable, trusted exact fixture commands and confined transcript capture to the output tree. |
| 7. Survivable evidence | Required fresh output, captured exact inputs and hashed raw artifacts before reporting success. |
| 8. Reliability | Preserved bounded peer traffic and exchange deadline, finite idle observation and process-group cleanup. Kernel-stall cleanup is not established by this harness. |
| 9. Tracking | Source qualification is complete; lifecycle dispatch remains the single active plan item. |
| 10. Verification | Actual three-case run, 38 corruptions, original three-case regression, refusal controls, Ruff and new-driver Black passed. |
| 11. Honest reporting | Kept configured-exit, finite-window and source-only limits explicit. |
| 12. Partnership | Qualified source behavior without treating source context as host control authority or reducing the commitment. |
| 13. Self-audit | Both independent reviews approved; no source-fixture finding remains open. |
| 14. Clear writing | The case table separates observed timing and context from host work still required. |
