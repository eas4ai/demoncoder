# Reserve unique session directories atomically under concurrent startup

Level: Judged
Decided by: agent
Rests on: HOOK-008,REL-002,VERIFY-006
Would be wrong if: Concurrent legitimate session opens collide, an existing record is reused or overwritten, or a lock/open/durability error is mistaken for a name collision.
History: The recorded private-source reversals showed that path checks alone do not establish safe storage or export boundaries. This remains Judged because it changes only fresh session-name reservation and preserves the existing pinned-parent descriptors, private modes, exclusive locks, validation and synchronization; it does not alter source/export policy or reuse historical records. Existing-candidate and later-error controls are required before accepting the repair.

## Decision

The persistence regression exposed timestamp-plus-process-ID session name collisions. A controlled current-library probe with 32 synchronized legitimate opens admitted only two and rejected 30 with an existing-directory error. Add a bounded Store::create_unique(parent,prefix) using numbered suffixes and atomic mkdirat under one pinned validated parent descriptor. Try at most 128 candidates and retry only the AlreadyExists result of directory creation. Once a new directory is reserved, initialize it exactly once using the existing guarded descriptors, private modes, validation, locks and durability synchronization; propagate any later error unchanged. Preserve Store::create exact-name behavior through the same initialization implementation, and retain the existing timestamp/process ID as the runtime prefix. Do not reopen, modify, follow or remove an existing candidate. No new random dependency or process-global counter is needed. Prove concurrent real opens, unchanged existing candidates, bounded exhaustion and no retry of later initialization errors. This repairs the runtime defect exposed by the allowance persistence prerequisite; keep its default-parallel regression checks and do not substitute a serial-only pass.

## Realized by

(none yet: recorded, not built)
