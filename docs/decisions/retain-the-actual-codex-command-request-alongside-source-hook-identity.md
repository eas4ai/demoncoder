# Retain the actual Codex command request alongside source hook identity

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-005,HOOK-008,HOOK-009,PRUN-001,PCOMP-003
Would be wrong if: A callback borrows another command or post-tool correction, historical receipts gain invented identity, or a transport acknowledgment replaces source ownership.

## Decision

Add an optional, defaulted command_request_id to durable SourceCallback metadata for the actual numeric JSON-RPC turn/start request sent by the Codex adapter. Keep Claude command_uuid absent for Codex; retain the native hook run ID as request_id, the private delivery UUID as envelope_id, and an ordered host occurrence sequence separately. None of these identities substitutes for another. Old receipts without the new field remain readable and do not gain authority. Bind each private callback to the active source hook notification, exact session and turn, and the adapter-owned outbound command. Remove only the private demonCoderOrdinary transport envelope before closed upstream input validation; preserve its identity in separate callback metadata. Extend the existing post-tool correction owner check to CodexDynamic: before the exact turn/start reply, only the reserved invocation with the same source session and admitted correction frame may submit; after acknowledgment require the exact retained thread, turn and numeric request ID. Keep correction content, allocation, deadline and one-use delivery admission bound to the original post-tool operation. Prove wrong or stale request IDs, cross-session or cross-turn callbacks, replayed deliveries and unrelated correction frames fail, while actual mixed post-tool correction works. This records the backend-specific representation of the existing owner decision and adds no new execution authority.

## Realized by

(none yet: recorded, not built)
