# Freeze private ordinary Codex hooks beside the managed compaction boundary

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-005,HOOK-008,HOOK-009,PRUN-001,PCOMP-003
Would be wrong if: Ambient hooks enter managed execution, a failed callback permits another model request, or ordinary registration weakens compaction or authentication policy.

## Decision

Add a separately retained patch after the existing pinned managed-compaction patch. Load a strict, bounded, hash-bound private UserPromptSubmit and ordinary Stop declaration once at process startup and preserve its immutable owner through configuration refresh. Preserve ambient hook suppression, existing authentication storage and non-hook managed access policy; do not use debug policy bypasses or relocate credentials. Keep declaration, source occurrence correlation and event-specific outputs separate from compaction challenges. Required ordinary callback launch failure, timeout, malformed output or uncertain acknowledgment must hold before model continuation, with source-specific prompt context and bounded Stop feedback retained. Preserve ordinary upstream behavior outside explicitly selected managed mode and do not dispatch ordinary Stop on synthetic memory or subagent events. Demonstrate actual-backend denial, bounded correction, cancellation, ambient canaries, refresh and declaration mutation, and failure cases. Bind new source/patch/binary hashes and rerun existing compaction and model-hook isolation qualification before using the new artifact as qualified. This extends the selected complete commitment, not its scope or acceptance criteria.

## Realized by

- 122aa6e934dff272d8dfa0f3bb511a493a27a2cf Add immutable private Submit and Stop boundary to managed Codex
