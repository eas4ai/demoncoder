# Fund native session model hooks from their original explicit grant

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-007,HOOK-008,PRUN-001
Would be wrong if: A session hook borrows task funding, spending outlives its original live owner or cumulative grant, shutdown replenishes limits, an external hook consumes unrelated delegation counts, or persisted attribution grants execution.
History: Earlier private-source reversals require preserving real source identity and explicit authority. This implements the already recorded session allowance policy within native lifetime ownership; it does not add backend-private lifecycle access or weaken source qualification.

## Decision

Enable synchronous Prompt and Agent hooks at actual native SessionStart and SessionEnd only when their original native lifetime owner holds the explicitly configured session grant. Keep ordinary task/delegation hooks on their original task reference, with no exhausted-task or unfunded fallback. Resolve preparation, live request/delivery, model/backend admission and host-observed snapshot tool admission against the same exact reference. Bound work and cleanup by the minimum of the existing live occurrence deadline, the original cumulative session deadline and the configured handler timeout; SessionEnd grants no new budget. Preserve exact key, pending invocation, owner, cancellation, snapshot, confinement and recovery checks. A persisted budget or operation cannot recreate the live lifetime capability. Native calls and host-admitted external backend invocations each consume a model slot; record session backend invocations separately from task/delegation counters, preserving existing task limits and unknown backend-internal usage. Stage counter overflow checks before spending. Retain isolated read-only Agent tools, tool-free Prompt execution, exact usage settlement and owned adapter close/cancellation through the existing runner lease. Task completion, cancellation or replacement cannot transfer or reset session funding. Held or expired owners admit no new effects and shutdown remains bounded. Test actual no-prompt startup/end flow, both native provider protocols and controlled external hook backends, request/effect/counter negative controls, snapshot tool debits, concurrency, persistence and cleanup. Preserve HTTP/MCP and asynchronous session-command denials for their subsequent lifetime integration. Native host lifecycle execution does not qualify external backend lifecycle sources or complete the commitment.

## Realized by

(none yet: recorded, not built)
