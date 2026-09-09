# Keep live checks on the selected connection with explicit per-run models

Level: Judged
Decided by: Codex
Rests on: CODE-010 CONN-001
Would be wrong if: A model override changes saved defaults, credentials, billing route or endpoint, or a quota response is mislabeled as a completed coding or Oracle cycle
History: Prior private-export decisions were reversed after narrower root assumptions failed. This verification-only choice remains Judged because it changes no production access boundary and follows the developer clarification; it preserves exact authentication and records the actual per-run model instead of silently substituting it.

## Decision

The developer clarified that a model-specific quota response confirms the authenticated connection is reachable and must not block connection verification. Preserve saved settings and authentication; use an explicit per-run model for actual live coding and Oracle cycles. The coding runner already supports this override. Add the same explicit option to the verdict-only Oracle runner and retain the actual selected model in its evidence. Keep completed-cycle evidence distinct from reachability and quota observations.

## Realized by

(none yet: recorded, not built)
