# Schedule dependencies and supervision inside the existing delegation owner

Level: Judged
Decided by: Codex
Rests on: ORCH-001 ORCH-002 ORCH-003 ORCH-004 ORCH-005 ORCH-006 ORCH-007
Would be wrong if: A queued dependency can start before validated developer integration, a role bypasses admission, or a restart replays uncertain work

## Decision

Extend the existing delegation manager with durable waiting assignments and a bounded dependency predicate over earlier assignment IDs. Reserve active capacity atomically before worktree preparation; start dependents from the parent content after explicit integration. Use the selected reviewer as a tool-free advisor after completed worker work and executed checks. Preserve a tool-free response under the worker connection and an independently selected tool-free judge decision. Reuse the same confined child session for at most two admitted correction turns, then rerun actual checks and supervision. Every role keeps its own labeled phase, context and retained evidence, while the existing runtime owns shared admissions, cancellation and recovery. No daemon, recursive delegation or automatic integration is added.

## Realized by

143195ae89ed53554b660e158a86e34d3b7bcb00
