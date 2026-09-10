# Shared plugin runtime and backend qualification order

Level: Judged
Decided by: agent
Rests on: EXT-001,EXT-007,PRUN-001,PCOMP-003
Would be wrong if: A plugin effect bypasses existing tool authority or an external backend crosses a denied lifecycle boundary.

## Decision

Implement immutable package imports and a shared typed lifecycle dispatcher attached to the existing ToolExecutor and SharedRuntime. Preserve the existing allocation ledger, effect receipts and explicit developer controls. Qualify actual Claude and Codex compaction barriers before enabling backend-dependent packages; a narrow pinned Codex integration may be required by PCOMP-003. Build one complete commitment in dependency order: import and validation foundation, backend barriers, final-candidate dispatch and confined runners, durable activation and recovery, skills and components, management UI, full conformance and live fixtures. No intermediate stage satisfies Done.

## Realized by

(none yet: recorded, not built)
