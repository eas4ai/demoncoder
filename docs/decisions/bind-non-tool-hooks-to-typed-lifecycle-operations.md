# Bind non-tool hooks to typed lifecycle operations

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-005,HOOK-007,HOOK-008,HOOK-009,HOOK-011,PRUN-001
Would be wrong if: A non-tool hook requires a fabricated tool or backend fact, loses durable ownership, or obtains another allocation for a Stop correction.

## Decision

Extend the existing Operation and hook receipt ledger with typed host lifecycle occurrences and a shared reserved-hook view. Preserve tool admission proofs and old serialized records. Implement UserPromptSubmit and ordinary Stop within the existing owned worker turn first, before the task is marked stopped or its phase ends. Retain a blocked prompt and its visible reason; bounded Stop corrections use only the original task allowance and correction limits. Cancellation and shutdown retain priority. Reuse existing runners, once reservations, observer ownership and confinement. Source framing requires actual observed source facts and cannot manufacture backend session, transcript or turn identifiers. SessionStart, SessionEnd, StopFailure, Interrupt and the remaining event matrix follow within this same commitment; this prerequisite does not establish full conformance or Done.

## Realized by

(none yet: recorded, not built)
