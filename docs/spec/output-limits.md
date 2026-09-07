# Model output limits

Status: Agreed 2026-09-06
Prefix: OUTPUT

The developer reported that the fixed 4,096-token native Anthropic output limit
makes the coding tool unreliable. Output limits must follow the selected model
and explicit developer settings, with truthful handling of truncated responses.

[OUTPUT-001] Native Anthropic requests MUST default to the selected model's provider-reported maximum output tokens, without a fixed harness ceiling. An explicit positive max_output_tokens setting MUST override discovery. Native OpenAI requests MUST retain provider defaults when no explicit limit is selected.
Falsifier: An uncapped modern-model fixture receives max_tokens 4096, a saved or explicitly overridden limit is ignored, a provider default is replaced by a guessed constant, or model discovery silently substitutes another model or credential.
Mechanism: output-limits; inspect actual requests through the production native adapters, model metadata discovery, explicit overrides and normal connection selection.

[OUTPUT-002] A provider response stopped by its output or context limit MUST remain visibly incomplete. It MUST NOT admit any tool calls from that response. Reported usage MUST remain visible. A later prompt MUST be usable without an invalid pending-tool history.
Falsifier: A truncated response produces a complete turn, its tool calls have effects, reported usage disappears, or the next prompt carries unexecuted tool-use records without results.
Mechanism: output-limits; controlled truncated text and tool streams through the production session, followed by a successful continuation.

[OUTPUT-003] Missing or invalid required model metadata MUST produce an actionable limitation, with an explicit limit available for compatible endpoints. Discovery MUST be cancellable and must not expose credentials. Unsupported explicit output-limit settings on external backends MUST be rejected rather than ignored.
Falsifier: Missing metadata silently falls back to a guessed cap, cancellation cannot stop waiting for discovery, metadata redirects leak authentication, malformed metadata reaches a message request, or a backend silently ignores a requested limit.
Mechanism: output-limits; discovery failure, invalid metadata, cancellation, configuration and backend-boundary cases.

Model output limits are distinct from context-window size, tool-result byte
limits and cumulative task budgets. This correction does not add the later
cumulative-budget or context-compaction features.
