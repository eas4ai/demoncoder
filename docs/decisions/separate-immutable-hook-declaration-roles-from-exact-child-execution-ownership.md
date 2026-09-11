# Separate immutable hook declaration roles from exact child execution ownership

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-005,HOOK-008,PRUN-001
Would be wrong if: A package role string grants child authority, a sibling or changed assignment reuses a reservation, or a child correction spends parent correction authority.

## Decision

Keep imported declarations and runner identities immutable. For native non-tool child execution, retain the exact child execution phase separately from the host-bound logical declaration role. AdmissionKey keeps the exact execution phase; the reserved declaration must match the logical role derived by the host from the admitted assignment. Bind child facts to the existing assignment identity, request, worktree and owning allocation relationship, and revalidate them through the existing child owner resolver before admission or correction. Do not fingerprint mutable spending as a new allowance or create a second ledger. Old records without child ownership proof cannot acquire child authority through defaults. Reuse the existing child correction limit and stage transitions; an unsupervised child with no correction ledger remains held rather than borrowing parent correction authority. Exercise actual Manager child execution and deny sibling, wrong-phase and changed-owner reuse.

## Realized by

7fd8d0b2734abf709bc1c6c42d3fdd9da7ac8805 — Exact child assignment, worktree,
allocation and correction ownership passed actual Manager and async delivery
cases, including sibling and changed-owner refusal. Specification and quality
reviews approved the bounded change. Explicit child MCP provisioning remains
separate managed-service integration work.
