# Observe native provider failures before ending their original turn

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-005,HOOK-008,HOOK-009,HOOK-011,PRUN-001
Would be wrong if: An observer converts a provider failure to success or correction, invents a failure for another operation, loses the original turn owner, or gains another allowance.

## Decision

Add native StopFailure as the next bounded lifecycle prerequisite. Capture only an actual returned provider failure after its model admission is settled, and dispatch the observation before finish_native_turn ends the existing durable turn. Preserve the original provider error, failure category provenance and known assistant text; unknown facts stay absent or explicitly unknown where the native contract permits. Reuse the existing typed occurrence, runner, receipt, once/observer ownership and original allocation rather than a new task or session authority. StopFailure is observation only: handler failure or cancellation cannot replace the original error, request correction, mark the turn complete or recursively emit another StopFailure. Do not classify arbitrary admission, hook, persistence or tool errors as provider failures. Respect remaining authority and cancellation; if an expired/held owner cannot execute an observer, retain a truthful diagnostic without borrowing an allowance or replaying later. Exercise production native turn failure with real hook effects, failing and malformed observer results, cancellation, exact ownership, original usage, no recursion and old receipt decoding. Imported source-shaped inputs use real native host facts and explicit provenance; this does not claim an external backend emitted a StopFailure callback. Outer allocation timeout that drops the native future, session startup/shutdown ownership, Interrupt and actual external source events remain required subsequent work in this same commitment. No full-conformance or Done claim follows from this prerequisite.

## Realized by

(none yet: recorded, not built)
