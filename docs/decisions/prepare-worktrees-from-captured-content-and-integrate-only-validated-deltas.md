# Prepare worktrees from captured content and integrate only validated deltas

Level: Judged
Decided by: Codex
Rests on: SUB-003 SUB-005 SUB-007
Would be wrong if: Preparing a child changes parent files, integration loses a parent edit, or restart repeats an uncertain integration

## Decision

Create genuine parent-owned Git worktrees and materialize the bounded captured parent baseline, including uncommitted and untracked content, without changing the parent index. Preserve separate child-root and Git administrative identities. Runtime-owned Git commands disable hooks and filters. Only a developer operation may integrate a current validated child delta within declared ownership; check touched parent paths against baseline and preserve unrelated edits. Retain child commits for inspection. Persist intent before every integration effect and require inspection after interruption without replay. Capture and Git freshness checks detect ordinary concurrent edits but do not claim an atomic filesystem transaction against arbitrary external writers; document that limit and require stable files during integration.

## Realized by

(none yet: recorded, not built)
