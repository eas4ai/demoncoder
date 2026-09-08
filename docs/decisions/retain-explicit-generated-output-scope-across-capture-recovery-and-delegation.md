# Retain explicit generated-output scope across capture recovery and delegation

Level: Judged
Decided by: Codex
Rests on: AUD-003 VERIFY-001 VERIFY-006 SUB-003 SUB-005
Would be wrong if: A scope change reuses prior evidence or grants recovery authority, an undeclared source edit is ignored, or excluded artifacts enter delegated Git objects

## Decision

Add repeatable generated-output paths selected at launch, bounded to explicit relative files or subtrees. Validate and canonicalize the list without following workspace links. Retain it in snapshot identity and session recovery metadata, keep default capture behavior for an empty list, and reject a resumed session with a different declaration. Apply the retained scope at every parent and child capture, verification, review and integration boundary. Excluded outputs are omitted before reading or exporting bytes, with visible scope; they do not become source changes or widen tool permissions.

## Realized by

(none yet: recorded, not built)
