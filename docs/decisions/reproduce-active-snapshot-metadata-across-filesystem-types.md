# Reproduce active snapshot metadata across filesystem types

Level: Judged
Decided by: agent
Rests on: PRUN-001,PRUN-002,HOOK-007
Would be wrong if: A staged snapshot loses ownership, permissions, ACLs, security attributes or an active statx attribute, or a changed source support mask escapes freshness validation.

## Decision

An ordinary ext4 workspace has no active statx attributes but advertises more optional filesystem features than tmpfs staging. Requiring identical support masks rejects that valid snapshot before its gate can run. Keep the complete original metadata, including the support mask, in capture identity and freshness comparisons. During materialization compare ownership, ACLs, extended security attributes and active statx attributes exactly, and require the destination to report support for every active source attribute. Inactive optional feature support may differ because it does not change the captured object access state. Preserve mode checks and fail closed for any active attribute or access metadata that cannot be reproduced. Demonstrate the original cross-filesystem failure, a successful ordinary snapshot, and rejection of changed active attributes and access metadata.

## Realized by

(none yet: recorded, not built)
