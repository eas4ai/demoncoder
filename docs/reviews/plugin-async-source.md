# Claude asynchronous command source qualification

Status: source qualification and independent specification/quality reviews passed.
This does not establish DemonCoder asynchronous execution or complete delivery.

The probe uses the actual pinned Claude Code 2.1.267 executable with isolated
HOME and configuration, a synthetic key, a local model peer and SDK MCP capture.
Executable SHA-256:
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`.

## Observed behavior

| Case | Model requests / results | Observation |
|---|---|---|
| First-line async, configured exit 0 | 5 / 2 | With config async false, the first stdout async marker transfers work; context first reaches request 3. |
| First-line async, configured exit 1 | 5 / 2 | The same delayed context arrives despite the configured unsuccessful exit. |
| Synchronous control, configured exit 0 | 5 / 2 | Without an async marker, context already reaches request 2. |
| asyncRewake true, configured exit 2 | 3 / 2 | One developer prompt produces a later model turn containing the command's stderr marker. |
| asyncRewake true, configured exit 0 | 2 / 1 | No extra turn occurs during the recorded idle observation window. |
| Ordinary async, configured exit 2 | 2 / 1 | No extra turn occurs during that same window. |
| asyncRewake true and async false, configured exit 2 | 3 / 2 | The flag still transfers work and starts a later turn. |

Each idle case checks one host prompt and one exactly correlated capture call.
The first foreground result precedes the command's end marker. A rewake request
follows that marker and keeps the same source session. Controls observe two
seconds after the first result and at least half a second after the end marker;
they do not prove indefinite absence of future work.

First-line cases request an async timeout of 100ms, while commands record their
end after a configured 400ms delay and contribute context. This source behavior
does not relax the host's finite original deadline. Likewise, source async
launch-time one-shot consumption does not replace the host's actual-success rule.
The probe records a marker before deterministic `sys.exit`; it does not capture
independent OS exit telemetry for the command.

## Repaired source-review finding

S1: the initial verifier checked delayed context but did not require the second
model request to precede first-line command completion, or follow completion in
the synchronous control. Reversing those recorded orderings was accepted in all
three cases. Actual captures have the expected order. The repaired verifier now asserts
model-request ordering against command completion and rejects reversed-order
controls for all three cases. The recorded RED run accepted all three corruptions;
the repaired replay and fresh source run passed. Independent re-review closed S1.

## Mechanism and failure controls

`tests/plugin_async_source_inputs.py` reuses the isolated peer and cleanup in
`tests/plugin_once_source_inputs.py`. An optional observer scenario adds the
first-line marker, idle observation and rewake configuration. Ordinary one-shot
calls keep the previous scenario, output and assertions.

The verifier reads retained raw hook logs, backend events, SDK requests, model
requests, inputs and command source. It checks source session and tool identity,
call metadata, exact prompt count, timing, result success and actual user-text
context. A marker in unrelated request metadata cannot stand in for delivered
context. Seven cases passed with 135 rejected corrupted-evidence variants.

The unchanged one-shot expectations also passed all seven cases and 48 corruption
controls using this same modified helper. The two runs share its exact hash.
Optimization-disabled assertions, a wrong executable and reuse of an existing
output directory were each refused. Both scripts passed Ruff and Black.

Final source artifact hashes:

- Helper: `2fcb097b0d077d512fabe42321fdde99e3400cd95078dfa82fb5405d87c2f682`.
- Async driver: `2adbe9ae2e5d902f0caf14907c18d18dedc951dcd76509f35723cc6029d33532`.
- Async fixture: `ddf27683c3073788b88b3bf872cc74ce78dc111a34f5cc8ebec13779b43f960b`.
- One-shot fixture: `b9a59fba59b8abb8376750bfbc1b4411fb8f83dbfd05a4112a12014792ea0113`.

The async run is retained at
`/home/shawn/demoncoder-check-tmp/claude-async source-4/qualification.json`;
its log is `claude-async-source-4.log` in the parent directory. The one-shot
regression is at `claude-once async-regression/qualification.json`, with log
`claude-once-async-regression.log`. The parent independently rehashed all 73
async artifacts, including captured inputs, and all 69 one-shot artifacts.
Earlier exploratory probes are not the final retained qualification.

Independent specification review approved the repaired four-file candidate,
followed by quality approval. Both independently replayed the checks and
verified retained hashes. Reports are `plugin-async-source-spec-review.md` and
`plugin-async-source-quality-review.md` in the same temporary parent directory.
Runtime ownership, delivery, cancellation,
restart, rewake admission and complete installed/live conformance remain separate
required work in the current commitment.


## Production self-audit for the source fixture

| Rule | Assessment |
|---|---|
| 1. Understand | Qualified actual first-line and idle behavior before deriving host expectations. |
| 2. Coherent change | Reused the established peer and cleanup through an optional observer scenario. |
| 3. Maintainability | Kept the new verifier separate and preserved default one-shot behavior. |
| 4. Boundaries | Checked exact inputs, source session, call metadata, prompts and result frames. |
| 5. Errors and secrets | Isolated synthetic credentials; failures retain diagnostics and cannot produce a passing report. |
| 6. Security | Pinned executable, cleared ambient environment and shell-quoted command paths, including spaces. |
| 7. Survivable evidence | Fresh output directory, captured inputs, raw evidence and hashes prevent stale success reuse. |
| 8. Reliability | Retained peer/process cleanup, bounded traffic, finite polling and an explicit idle observation window. |
| 9. Tracking | Source qualification is complete; lifecycle dispatch remains the single active plan item. |
| 10. Verification | Actual seven-case run, 135 corruptions, seven-case one-shot regression with 48 corruptions, Ruff and Black passed. |
| 11. Honest reporting | Source observations, finite-window negatives and missing independent command exit telemetry remain explicit. |
| 12. Partnership | Recorded the source behavior and host differences without changing the agreed delivery scope. |
| 13. Self-audit | Repaired the review finding and obtained fresh evidence and both approvals; no source-fixture finding remains open. |
| 14. Clear writing | The case table separates observed behavior, verification and host work still required. |
