# Give each native turn a durable identity before hook selection

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-008,PCOMP-002,PCOMP-003
Would be wrong if: Submit and Stop for one actual native turn expose different turn identities, a plugin-origin continuation becomes a developer submission, or the turn record creates another allowance.

## Decision

Record the actual native turn start in the existing durable operation store before selecting optional hooks. Retain its origin and owner, and use its stable identity through submit, Stop and bounded internal corrections until that turn ends. Stop-only configurations still have the same real turn fact; do not manufacture a UserPromptSubmit event for an observer wakeup merely to obtain an identifier. Source-format translation uses genuine native session, transcript, workspace, model, permission and turn facts, with explicit host-translation provenance. An imported dialect does not select a backend. External backend identities remain actual observations correlated with the host turn. This adds neither a task nor an allocation ledger, and cancellation preserves existing cleanup and uncertain-effect recovery.

## Realized by

(none yet: recorded, not built)
