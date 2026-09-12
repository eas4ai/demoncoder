# Persist explicit session hook allowances without task funding or resume resets

Level: Judged
Decided by: agent
Rests on: HOOK-007,HOOK-008,PRUN-001
Would be wrong if: Omission grants funding, a restart replenishes counters or time, changing a task changes the session grant, or session settings silently weaken existing task limits.

## Decision

Add an optional session-hook allowance to the existing durable runtime. Require the developer to supply all three invocation options together: --session-hook-seconds (1 to 86400), --session-hook-model-calls (0 to 4096), and --session-hook-tool-calls (0 to 4096). Omission grants nothing; package content and task defaults cannot supply it. Reuse the existing cumulative wall-clock and counter implementation through a separate validated session constructor, leaving task validation unchanged. Store the original explicit limits and allocation separately from task/delegation allocation. On resume, require the identical explicit options before changing recovery state; reject changed, removed or newly added grants with instructions to restore the original options or start a new session. Preserve saved counters, deadline, clock uncertainty and usage, and checkpoint both allocations. Task creation, replacement, archive and reconciliation cannot replenish or transfer session funding. Model slots will bound native calls and externally admitted backend invocations with separately labelled backend counts; tool slots cover host-observed Agent inspection tool admissions, not invented knowledge of remote internal tools. HTTP and MCP keep their existing time and service-local limits unless separately specified. Implement and verify durable configuration first, then exact budget attribution and runner admission as separate reviewed actions; this first prerequisite enables no new handler execution and does not complete lifecycle dispatch.

## Realized by

- 46122703dc052b0801f3be424382c4cb34eb0f12 Persist session hook allowances and prevent session name collisions
