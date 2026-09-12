# Settle child observers before advancing supervision

Level: Judged
Decided by: agent
Rests on: HOOK-008,HOOK-011,PCOMP-002
Would be wrong if: Ordinary child turn completion discards an admitted observer, rewake borrows parent authority, or draining prevents cancellation or exceeds the original allowance.

## Decision

Keep the existing child worker session and assignment owner active after ordinary foreground completion until its already-admitted observers settle or ownership is cancelled or expires. Drain only those bounded existing jobs under the original deadline; do not create a second scheduler or new allocation. Before advancing to validation or a checkpoint, atomically reserve any eligible explicit rewake from its retained receipt, exact child identity and remaining supervision correction ledger. Deliver attributed data directly to the same child session, preserve writer quiescence, and repeat only within the original correction and call limits. Child admission must neither depend on nor change the global parent phase. Ordinary async completion creates no idle model call; retain its context for a later eligible corrective child boundary until actual assignment termination. Explicit cancellation and changed ownership win queued rewake and retain uncertain effects. This refines the original-owner async decision and repairs specification finding S1 without weakening child supervision or the complete commitment.

## Realized by

- 0190b6ccb2fc4af111a87c8fa9798bc500c266f0 Own asynchronous hooks through completion and child continuation
