# Own delegation and integration in the existing session runtime

Level: Judged
Decided by: Codex
Rests on: SUB-001 SUB-003 SUB-004 SUB-005 SUB-006 SUB-007
Would be wrong if: A second coding loop advances a child, budgets reset, or child output gains developer authority

## Decision

Use a bounded delegation manager attached to the existing session owner and shared durable runtime. Reuse the four adapter factories for child execution, label child events and preserve their original results. Parent tools can request assignments and inspect agent evidence; only a developer control can integrate a validated child. Enforce deadlines and host tool admissions cumulatively, count native calls and external backend invocations as separately named controls, and keep unavailable backend-internal usage unknown. Interrupted backend children retain worktree and evidence for inspection without claiming opaque conversation restoration.

## Realized by

- 65bb87becd6b69709a52880448deb9de81561b9f Add bounded subagents across API and subscription connections
