# Release settings locks when their owning transaction ends

Level: Judged
Decided by: Codex
Rests on: SUB-002 SET-007 AUD-006
Would be wrong if: A second settings writer enters before the first transaction ends, or an inherited descriptor keeps a completed transaction locked

## Decision

Keep the existing nonblocking exclusive lock and revision conflict checks. Return a guard that explicitly unlocks when the owning setup or save transaction ends, including error paths, before closing its descriptor. A descriptor inherited during a concurrent process launch must not extend that transaction. Verify exclusion while the guard lives and immediate reacquisition after it drops with a duplicated open file description still alive; retain the competing-editor and active-revision assertions.

## Realized by

- 4397100306554a0fb6fe72f2395336529302f6ea Release settings transaction locks despite inherited descriptors
## Verification

The deterministic duplicated-descriptor regression failed before the guard change and passes with explicit unlock. It also checks that a live writer excludes competitors and that dropping the inherited descriptor cannot unlock a later transaction. Existing competing-editor and active-revision assertions remain intact.

Parallel library verification also exposed an executable-fixture race: a fork can temporarily retain the fixture writer and cause ETXTBSY. The test helper now checks fixture readiness, retrying only that OS error for at most two seconds before running the unchanged production probe. Production backend launch behavior and route validation are unchanged.
