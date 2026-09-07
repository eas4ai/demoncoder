# Keep task acceptance and recovery in the existing session owner

Level: Judged
Decided by: Codex
Rests on: VERIFY-001 VERIFY-002 VERIFY-003 VERIFY-004 VERIFY-005 VERIFY-006
Would be wrong if: The workflow duplicates an adapter loop, accepts stale evidence, or cannot durably block uncertain execution

## Decision

Extend the existing host session boundary with an explicit task workflow. Developer commands select checks and a tool-free reviewer, run verification, request bounded correction and accept the current workspace only after current checks and review pass. Use one private versioned record for task transitions, allocations, original results, decisions and native conversation checkpoints. Persist admissions before effects. Restore native adapter context without replaying interrupted calls; reject unsupported backend recovery or allocation guarantees visibly. Keep all checks on the existing ToolExecutor and all auxiliary model calls within the shared allocation. No database, daemon or second coding loop is introduced.

## Realized by

- d94e156fa76f467fd82e1c9977cb431ba225592b Add explicit task verification and durable recovery
