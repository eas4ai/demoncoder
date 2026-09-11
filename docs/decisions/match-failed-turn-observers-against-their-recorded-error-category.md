# Match failed-turn observers against their recorded error category

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-008,HOOK-009,PRUN-001
Would be wrong if: A matcher reads error prose or another turn instead of the recorded category, hangs evaluation, or changes existing declaration identities when unused.

## Decision

Add an optional error_category matcher only for StopFailure declarations. Omit the absent field from serialization so existing declaration bytes and digests remain unchanged. Compile its bounded pattern once with the existing jsonschema regex options and backtracking limit already used by wire validation; do not add a second regex dependency or reinterpret patterns as literal equality. Match only the immutable occurrence error category, not arbitrary provider details or previous-turn text. Reject unsupported event/field combinations, malformed or oversized patterns, and keep matcher evaluation bounded. Test matching, nonmatching and hostile patterns plus unchanged legacy serialization. This supplies native runtime matching; public source-package matcher binding remains part of the later activation/import integration and is not claimed complete here.

## Realized by

(none yet: recorded, not built)
