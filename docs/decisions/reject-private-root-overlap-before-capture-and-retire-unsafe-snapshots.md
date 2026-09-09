# Reject private-root overlap before capture and retire unsafe snapshots

Level: Judged
Decided by: Codex
Supersedes: exclude-private-source-before-capture-review-and-delegated-git-export
Cause: the stated condition occurred
Rests on: AUD-001 AUD-002 CODE-007 VERIFY-003 SUB-003 SUB-005
Would be wrong if: A declared private root reaches retained source or Git exports, an old unsafe baseline reaches review, or an unrelated public project is refused

## Decision

Share HOME-relative and environment-declared private roots with the tool boundary. Before any parent or delegated capture, refuse lexical or canonical overlap in either direction. Preserve reserved-name filtering within ordinary public projects and explicit settings admission. Increment the export-policy version and refuse older snapshots at review because their arbitrary private paths have no retained provenance; require a new task baseline without rewriting historical records or Git objects.

## Realized by

- 7a58899e1d2c4236bfdc5c7a9038254c7fe00fcf Reject private-root capture and unsafe historical review baselines

The shared private-root collector protects developer tools and the lower capture entry. Parent and delegated capture refuse overlapping roots before export; review rejects unsafe older versions. Focused production tests and terminal cases prove refusal and continued public review/integration.
