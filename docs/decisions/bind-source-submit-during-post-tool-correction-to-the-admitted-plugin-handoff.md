# Bind source Submit during post-tool correction to the admitted plugin handoff

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-005,HOOK-008,HOOK-009,PRUN-001,PCOMP-002
Would be wrong if: A machine prompt acquires developer origin, an unrelated callback clears a pending correction, or callback delivery creates another allowance or repeats a completed tool.

## Decision

Pinned Claude emits UserPromptSubmit for the host post-tool correction before its user-message replay acknowledgment. Permit only the registered ordinary callback bound to the live, already reserved ExternalCorrection, its original backend and post-tool operation, exact frozen outbound message UUID/content and observed source command-start/session. Retain explicit plugin-correction origin and causal post-operation identity in host facts; correction=true alone must not imply developer steering. Preserve the separate source prompt identity and event-specific extraction from the actual frozen content, including retained provider blocks. The source callback may settle its own existing lifecycle receipt but cannot clear the original handoff, deadline, pending delivery or replay requirement. Existing exact replay acknowledgment remains required before accepting subsequent source work; no generic bypass for hooks before acknowledgment. Use only the original correction allocation and preserve completed tool evidence. Demonstrate actual mixed-event pass/deny/cancel and forged/stale handoff refusal before claiming combined compatibility.

## Realized by

(none yet: recorded, not built)
