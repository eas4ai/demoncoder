# Authorize external non-tool callbacks through their actual lifecycle owner

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-005,HOOK-008,HOOK-009,PRUN-001,PCOMP-003
Would be wrong if: A forged or stale callback grants work, denial crosses a model boundary, or a backend Stop correction gains another task or allowance.

## Decision

Route actual Claude and Codex UserPromptSubmit and ordinary Stop callbacks through the existing typed lifecycle operation, runner, receipt and owner ledger. Preserve source session, turn and request identity separately from host occurrence identity. Keep compaction challenge acknowledgments and tool identity checks intact; ordinary events use their own tested correlation and output contracts rather than fabricated tools or compaction challenges. Retain protected relay credentials, bounded frames, replay checks and owner-supervised cancellation. Host gate disposition remains authoritative even when a source reports transport completion. Admit Stop continuation against the original correction limit, deadline and cumulative allocation before releasing the source callback. Verify actual pinned backends against controlled model peers for allow, deny, correction, cancellation, malformed and forged callbacks, and uncertain recovery. Source-only probes do not establish host delivery; external transcript facts retain their actual provenance. Any additional backend patch or authority mechanism needs its own recorded assessment before implementation.

## Realized by

(none yet: recorded, not built)
