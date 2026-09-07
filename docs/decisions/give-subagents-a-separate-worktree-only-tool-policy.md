# Give subagents a separate worktree-only tool policy

Level: Judged
Decided by: Codex
Rests on: SUB-002 SUB-003 SUB-004
Would be wrong if: A parent host flag, Oracle allow decision or backend built-in tool grants access outside a child worktree

## Decision

Construct child access independently of the parent. Route API and subscription child coding tools through the existing host executor with a dedicated restricted policy. Pin the worktree root, deny Git administrative paths, and sandbox shell execution with only the child worktree writable and minimal read-only system runtime dependencies. Do not mount user home, parent worktrees, private records or shared Git administration into the child tool view. Backend login remains with the trusted transport; backend built-in coding tools stay disabled. Failure to establish confinement blocks execution, with no host fallback. Test home movement, deletion, overwrite and path-alias attempts using synthetic canaries.

## Realized by

94cd704af806fee84f8712d1c5b09aefb39cd048 Confine child tools to their worktree
962010ed71e03d3d897dc3a6a8b63510c65195ea Reject mount crossings in strict worktree tools
