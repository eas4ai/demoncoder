# Keep complete changed source while bounding supporting review context

Level: Judged
Decided by: Codex
Rests on: AUD-004 VERIFY-003 SUB-005
Would be wrong if: Changed source or omitted-source identity is silently dropped, omitted contents are labeled reviewed, or a changed review scope reuses prior acceptance

## Decision

Add repeatable --review-context paths and a mutually exclusive --review-changes-only option. Omission preserves the existing whole-baseline review. Retain the review-context selection alongside generated-output scope in snapshot and recovery identity so parent and child review inherit the same declaration. Continue capturing every in-scope source input. Include complete old/new changed source regardless of the context selection, include selected unchanged context, and identify every omitted entry by path and content metadata with an explicit not-reviewed label. Refuse before the reviewer request if mandatory content or omission identities exceed the evidence limit.

## Realized by

(none yet: recorded, not built)
