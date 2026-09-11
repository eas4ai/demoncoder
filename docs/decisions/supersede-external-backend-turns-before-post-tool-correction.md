# Supersede external backend turns before post-tool correction

Level: Judged
Decided by: agent
Rests on: HOOK-004,HOOK-008,HOOK-011,PRUN-001,PCOMP-002
Would be wrong if: A backend starts more work before its correction boundary settles, the completed result is lost, an uncertain handoff replays, or a correction receives fresh authority.

## Decision

When a native post-tool handler requests correction on an external backend, retain the completed host result and source identity, then withhold the pending dynamic-tool response or post-tool callback. Persist a superseding state before interrupting the backend and refuse further tool admissions. Require both the correlated interrupt acknowledgment and the qualified terminal result before marking that backend turn superseded. Codex must report the same turn interrupted; pinned Claude must report its aborted-tools result. Neither signal alone releases work. Revalidate the retained inspection and original owner, then atomically reserve the next backend invocation and consume one correction from the existing task and allocation. Send a bounded prompt separating immutable host completion evidence from attributed plugin feedback and replacement output. Record the correction handoff only after the backend acknowledges the new invocation; never call the withheld old result delivered. Cancellation, stale inputs or exhausted authority before reservation spends no correction. Failed or interrupted transmission after reservation remains uncertain, blocks further admission and never replays automatically. The same receipt owns superseding, superseded, correction-reserved and correction-acknowledged states. Ordinary admission cannot bypass intermediate states. Pinned controlled-source probes establish the interruption sequence; production tests must prove actual retained mutations, allowance ownership and fault behavior. This implements the already selected synchronous correction contract and does not reduce the complete commitment.

## Realized by

(none yet: recorded, not built)
