# Keep asynchronous hook work with its original owner and allowance

Level: Judged
Decided by: agent
Rests on: HOOK-006,HOOK-008,HOOK-011,PRUN-001,PCOMP-002
Would be wrong if: An observer outlives revoked ownership, borrows a new task allowance, consumes once at launch, repeats an uncertain effect, or retroactively authorizes completed work.

## Decision

Keep asynchronous observers in the existing session runtime and durable hook ledger. A bounded live job retains its exact reserved invocation, original task or child, immutable package and policy, remaining allocation, absolute deadline and supervised process resources. Ordinary foreground completion closes new admission but does not cancel already transferred observer work. Explicit cancellation, shutdown, owner replacement, changed authority and deadline exhaustion revoke that work. Use weak runtime ownership and bounded cancellation/join handles; an event sink must not keep the owner alive. Do not introduce a second job database or grant a new allocation.

Transfer only an admitted observer. A required gate cannot be satisfied by scheduling. Support declared async and source-supported first-line async output inside the existing bounded supervised reader, before process exit. Preserve cancellation fairness and final output separately from the launch marker. Requested timeouts can shorten the original deadline. Keep confinement, capacity and mutation guards through actual teardown, including blocking cleanup.

Persist transfer and actual completion separately from tool settlement. Launch is not one-shot success. Consume only after the exact valid successful outcome; interrupted effects remain unknown and never replay after restart. Retain bounded attributed context and delivery references without rewriting completed tool evidence or supplying old gate authority. Delivery occurs at safe model boundaries, with durable reservation before sending; uncertain delivery is not automatically repeated. Command-shaped plugin text cannot invoke developer controls.

Ordinary async completion while idle waits for the next eligible model boundary. Source-supported explicit rewake uses internal admission under the original owner and remaining allowance, never a fresh task, unbounded correction or developer acceptance. Cancellation wins a queued wake. Quiesce admitted writers before verification, review, acceptance and owner replacement. Exercise native and external loop owners with actual effects, downstream requests, cancellation, restart, saturation and weak-owner failure controls. This advances the existing complete commitment; additional lifecycle events, public package controls and full conformance remain required.

Retain required-policy status as an explicit host-owned fact, separate from a
handler class that can produce both decisions and observations. A non-required
source command may request first-line transfer; required policy remains a gate
for every dialect. Output cannot change that status. Preserve the old required
semantics when historical records lack the new field, and include this fact in
the captured policy identity used for reuse and settlement.

## Realized by

- 0190b6ccb2fc4af111a87c8fa9798bc500c266f0 Own asynchronous hooks through completion and child continuation
