# Resolve output limits from provider metadata and explicit settings

Level: Judged
Decided by: Codex
Rests on: OUTPUT-001, OUTPUT-002, OUTPUT-003 and the reported fixed native output cap
Would be wrong if: Modern models remain capped by a guessed constant, explicit limits are ignored, or truncated tool calls execute

## Decision

Use lazy, cancellable model metadata discovery for the required Anthropic max_tokens value and cache a valid result for the session. An explicit positive max_output_tokens setting bypasses discovery for compatible endpoints and is supported by both native API adapters. Omitted OpenAI limits retain provider defaults. Reject unsupported external-backend overrides. Surface provider token-limit stops as incomplete with usage retained and no tool admission. Validate this through actual protocol requests and session outcomes.

## Realized by

(none yet: recorded, not built)
