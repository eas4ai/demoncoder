# Mark provider response failures at actual adapter transport and protocol boundaries

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-008,HOOK-009
Would be wrong if: A local presentation, persistence, validation or configuration error emits StopFailure, or classifying a provider error changes its original error chain.

## Decision

Keep the Model::response signature and add an explicit ProviderResponseFailure marker around errors produced at actual OpenAI and Anthropic transport or provider-protocol boundaries. Native failure observation requires this marker; ordinary unmarked errors do not emit StopFailure. Unwrap the marker before returning so the original provider error and downcast identity remain available. Mark transport/status, stream decode and provider error or incomplete/invalid response failures at their origin. Leave EventSink emission, event persistence, local configuration and hook-request validation errors unmarked. Synthetic providers must explicitly mark their simulated provider failure. Test the real adapter with a closed terminal receiver before any HTTP request, local event failure after successful provider output, and actual transport/protocol errors; observe request counts, observer effects and original error identity. This resolves the discovered ambiguity without a trait-wide result migration, message matching or a broad inference that every model-method error came from the provider.

This includes actual provider metadata transport/protocol failures reached while
preparing a response. Local output-limit or configuration rejection remains
unmarked. Preserve existing configuration guidance as context over the retained
cause; do not flatten a provider failure into a new string or lose outer guidance
when extracting the marker.

Separate HTTP request construction from execution. Invalid credential headers
and other local builder failures stay unmarked and sanitized. Mark only actual
transport and provider-response failures after that boundary; preserve the
existing generic HTTP helper's behavior for its other callers.

## Realized by

(none yet: recorded, not built)
