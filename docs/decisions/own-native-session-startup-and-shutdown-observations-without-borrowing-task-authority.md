# Own native session startup and shutdown observations without borrowing task authority

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-005,HOOK-008,HOOK-010,HOOK-011,PRUN-001
Would be wrong if: Resource close fabricates SessionEnd, a session observer reopens stopped or held task work, repeats uncertain effects, spends an implicit model allowance, or delays shutdown without a whole-boundary bound.

## Decision

Add native host session lifetime identity to the existing durable operation ledger, and wire bounded command observations at actual outer session startup and final termination. Distinguish startup from resume and retain the real end reason. Resource close after turn failure, cancellation or provider replacement is not session termination. Forward boundary entrypoints through existing wrappers without duplicating parent or child events. Preserve task/delegation allocation and worker-turn validation; a typed session lifetime owner permits only its admitted session observation, never task continuation, correction or model spending. Use a host-owned whole-boundary cap of 30 seconds for startup observation and 5 seconds for end observation, including waits, handler execution and cleanup; reuse the existing process cleanup reserve within that cap. Command declaration limits may shorten these caps, never extend them. Command lifetime observation has no new model allowance and must not reset or charge ordinary task/delegation counters. Persist exact lifetime, workspace, execution identity, occurrence and uncertain outcome, with backward-compatible records and no automatic replay. Recovery holds or persistence failure cannot authorize new effects; retain truthful unavailable observation diagnostics when possible and continue shutdown. Preserve command-channel cancellation priority, reject queued work on shutdown and always complete ordinary resource cleanup even if observation fails. Prove startup/end without any submitted prompt, repeated resource close versus actual end, cancellation/error followed by another prompt, bounded stalled handlers/permits, exact ownership, stopped and recovery-held tasks, old records and restart without replay using actual confined command effects. This is a native lifetime/command prerequisite only. SessionStart side effects beyond this bounded observation, Interrupt, outer allocation timeout, source backend callbacks, and HTTP/MCP/model session execution remain required in the same commitment. Those runners need explicit session allowance and typed budget/service ownership; do not grant them task defaults or claim full lifecycle conformance here.

Keep the lifetime fact in the outer event sink/runtime so replacing the inner
provider cannot lose final termination. Native observer effects require the
pinned startup identity and policy; a changed identity suppresses them with a
diagnostic rather than granting stale authority. Non-command session execution
stays unavailable even when a task or delegation allocation happens to exist.

This prerequisite executes synchronous native command registrations only. A
configured asynchronous or asynchronous-rewake command stays unavailable with a
diagnostic; silently running it synchronously would change its declared behavior.
Session-owned asynchronous execution and any permitted rewake need explicit
lifetime policy and remain required in the same commitment. They cannot borrow
task-context delivery or task continuation authority.

Preserve the actual host control that ends the session, including when a turn
consumes channel closure or shutdown during failure observation. Retain a selected
cause with the existing outer lifetime when needed; it grants no authority and
is not an early end fact. Keep the original provider error unchanged.

## Realized by

(none yet: recorded, not built)
