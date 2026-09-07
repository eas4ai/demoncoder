# Prepare worktrees from captured content and integrate only validated deltas

Level: Judged
Decided by: Codex
Rests on: SUB-003 SUB-005 SUB-007
Would be wrong if: Preparing a child changes parent files, integration loses a parent edit, or restart repeats an uncertain integration

## Decision

Create genuine parent-owned Git worktrees and materialize the bounded captured parent baseline, including uncommitted and untracked content, without changing the parent index. Preserve separate child-root and Git administrative identities. Runtime-owned Git commands disable hooks and filters. Only a developer operation may integrate a current validated child delta within declared ownership; check touched parent paths against baseline and preserve unrelated edits. Retain child commits for inspection. Persist intent before every integration effect and require inspection after interruption without replay. Capture and Git freshness checks detect ordinary concurrent edits but do not claim an atomic filesystem transaction against arbitrary external writers; document that limit and require stable files during integration.

## Realized by

- 3ad5bf0003d3402ac460366db26fc847d3b8cb19 Add isolated worktree snapshots and validated delta integration
- af8f6c6b8de692a781a3b217b8ea2ddeeab034a7 Make worktree filesystem work cooperatively cancellable
- 663dd3ebb3f8097b7e59cc4498585bbd9af78904 Pin Git delta paths before applying child results
