# Reserve outer shutdown time for native lifetime observations and resource cleanup

Level: Judged
Decided by: agent
Rests on: HOOK-004,HOOK-005,HOOK-006,HOOK-008,PRUN-001
Would be wrong if: The application aborts admitted native end observation or owned cleanup before its bound, quit queue submission can wait without a bound, or external-only sessions inherit extra native observation time.

## Decision

The existing application shutdown wrapper allows three seconds for queue submission and worker cleanup, but native session-end observation now owns up to five seconds including command cleanup. Reserve both budgets in the actual application path: eight seconds for a session that opens a native host lifetime, while external-only sessions retain the existing three seconds. Reuse the native end-bound constant so the inner and outer limits cannot drift. Keep queue submission and worker completion within the same outer deadline and retain abort-and-join behavior on timeout. If needed, move only the existing main shutdown sequence into a shared production helper so real confined end-command and delayed-resource-cleanup tests exercise exactly the path main calls. Preserve the five-second observation bound, original errors and selected end reason. This is an integration repair to the verified lifetime prerequisite; explicit session-hook budgets and remaining lifecycle behavior stay required in the same commitment.

## Realized by

(none yet: recorded, not built)
