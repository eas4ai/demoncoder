# Assignable subagents implementation plan

> Execute with superpowers:subagent-driven-development, one bounded component
> at a time with specification and quality review. Cairn remains the referee.

**Goal:** Deliver SUB-001 through SUB-007 in the installed terminal application.

**Architecture:** One delegation manager owns child scheduling and delegates each
child turn to an existing adapter. One authoritative private runtime record owns
assignment state and cumulative admissions. Child tools receive a separate
worktree-only access policy. The parent owns Git operations and validated merges.

**Tech stack:** Rust, Tokio, existing adapter registry, serde/private store,
Linux openat2 and bubblewrap, Git and Python production PTY fixtures.

## Task 1: strict child tool policy (SUB-002)

Files: modify src/tools.rs; add src/worktree_access.rs; export through src/lib.rs;
add tests/worktree_access.rs. Reuse existing bounded execution and socket filter.

- [x] Add a failing child-policy test using a synthetic sibling home and a
  linked worktree `.git` file. Attempt read/write/edit and Bash movement,
  recursive deletion, overwrite, symlink escape and administrative replacement.
  Assert the outside files and Git pointer remain byte-identical.
- [x] Add a runtime-only policy constructor with this contract:
  `AccessPolicy::worktree_only(credential_paths: Vec<PathBuf>) -> AccessPolicy`.
  It sets strict confinement, no Oracle and no unrestricted execution. Conflicting
  flags fail closed; ordinary default and host policies keep their existing rules.
- [x] Route strict reads through the pinned root; deny `.git` components for
  every file mutation and hard-linked files that can alias outside content.
  The strict Bash launcher exposes the root and minimal system binaries/libraries,
  masks `.git`, excludes home and private records, blocks Unix-socket escape,
  and uses existing output/deadline/cancellation handling without host fallback.
- [x] Run `cargo test --locked --test worktree_access`, host/developer access
  regressions, format and Clippy. Review spec first, then code quality; merge
  the reviewed component through the parent loop.

## Task 2: real worktrees and parent-owned integration (SUB-003, SUB-005)

Files: add src/subagents/worktree.rs and tests/subagent_worktrees.rs; extend
src/workflow/workspace.rs only for bounded raw baseline materialization/reuse.
Extract a shared hardened Git command helper from src/status.rs if appropriate.

- [x] Test a disposable repository with staged, unstaged, deleted, executable,
  binary, ignored and untracked baseline files. Capture original contents/index.
  Assert a genuine new worktree starts with that content and parent stays intact.
- [x] Implement parent-owned preparation with explicit stored identity:
  `PreparedWorktree { root, git_dir, common_dir, base_commit, baseline }`.
  Record intent before Git effects. Disable hooks, filters, signing and user Git
  environment; bound output/time. Pin roots and reject unsafe administrative paths.
- [x] Implement a validated delta manifest naming old/new content and owned paths.
  Reject changes outside ownership, stale child evidence and changed Git identity.
  Never parse arbitrary patches manually; generate/check with hardened Git.
- [x] Apply only the validated child delta on an explicit parent command. Check
  touched parent paths against their assignment baseline, preserve unrelated
  changes, reject conflicts without force/reject files, and retain child commits.
  Persist integration intent before effects and completion only after verification.
  Ordinary external writers remain subject to documented capture/freshness limits;
  an interrupted application never automatically replays integration.
- [x] Test actual success and conflict, malicious hooks/filters, stale receipts,
  outside ownership and interrupted intent. Run targeted Rust tests and both reviews.

## Task 3: assignment state and shared admission (SUB-001, SUB-006, SUB-007)

Files: add src/subagents/{mod,state,manager}.rs; extend workflow/runtime.rs,
events.rs and config.rs. Use the existing store rather than another database.

- [x] Test serialized assignments and transitions. Core record fields are id,
  objective, context, connection identity, owned paths, checks/reviewer, worktree
  identity, status, original results, validation snapshot and integration intent.
  State transitions reject running/stale/unvalidated integration and uncertain replay.
- [x] Add optional backward-compatible child records to the parent durable record.
  Use atomic updates to consume shared native call/tool counters and separately
  named backend invocation allowances before execution. All children share the
  parent's absolute deadline; no phase/restart refreshes it.
- [x] Bound active children (default two), queued work, retained messages/results
  and assignments. Enforce child-specific and shared remaining limits. Unsupported
  backend-internal token/cost/call controls remain visibly unavailable.
- [x] Persist every assignment admission, result, checkpoint and validation before
  publishing success. Restore native checkpoints only through existing contracts;
  interrupted opaque backends remain inspectable and stopped without a replacement.
- [x] Test exhaustion and restart before effects, old records, changed identity,
  disk write failure and sender/assignment-preserving message serialization.

## Task 4: adapter and terminal delegation path (SUB-001, SUB-004)

Files: extend tools.rs, session.rs, native.rs, adapters/{codex,claude,openai,
anthropic}.rs as needed, config.rs/main.rs, terminal.rs and events.rs.

- [x] Add a host-owned optional tool extension interface shared by all adapters.
  Parent delegation tools submit validated assignments to the manager. Child
  policies never expose delegation or integration tools. Default ordinary sessions
  retain the existing four-tool surface when delegation is not enabled.
- [x] Resolve developer-selected named child connections from trusted settings,
  preserving model/effort and authentication. Open children with existing factories
  using fresh strict policies and their own worktree/context, never inherited host
  access or child-provided connection configuration.
- [x] Add developer controls for listing/inspection, cancellation, validation,
  reconciliation and explicit integration. Mark them as controls so active turns
  cannot reinterpret them as worker corrections. Keep the editor responsive while
  children and parent run independently.
- [x] Label every child event with assignment and connection. Collect bounded
  original activity/result/usage and present it as agent evidence. A parent request
  can inspect results; only a developer control can authorize integration.
- [x] Cancel one child without affecting siblings; parent cancel/shutdown stops all.
  Use one lifecycle owner and existing backend/subprocess cleanup. Test delayed
  children and effects stopping within two seconds on all four transports.

## Task 5: validation and production proof (SUB-001 through SUB-007)

Files: add tests/assignable_subagents.py and scripts/check-assignable-subagents.sh;
reuse tests/verification_workflow.py, terminal_session.py and backend_fixture.py.
Update README.md with exact controls, capability limits and recovery behavior.

- [x] First run a terminal fixture requesting delegation on the baseline and retain
  the observed missing-command/tool failure. Then drive native and subscription
  children through actual read/edit/Bash effects in isolated worktrees.
- [x] Run child checks through the strict executor and existing tool-free reviewer
  with actual child baseline/patch/source/results. Require a current passing check
  set and clear review; blocked/invalid review and stale capture prevent integration.
- [x] Exercise parent independent work, individual/parent cancellation, budget
  exhaustion, synthetic home attacks, authentication selection and labeled results.
  Run clean integration and conflicting parent edits; parent acceptance must become
  unverified after a merge. Kill/restart at child and integration checkpoints.
- [x] The shell mechanism emits `cairn: SUB-00N: pass` only after that requirement's
  production cases and determining component checks succeed. Missing support is
  failing evidence, never a skipped success. No paid requests occur by default.
- [x] Run format, Clippy, full Rust tests and documented affected terminal drivers.
  Commit implementation before Cairn checks, then commit all receipts/output files.

## Release gate

Record completion in .cairn/reviews/assignable-subagents.md against the final
committed candidate. Review ownership, confinement, cancellation, stale merges,
allocation and interrupted recovery without changing code; record findings before
separate repairs. Perform the 14-rule audit. Install the release, compare the
installed executable to the build, and run production subagent workflows against
that executable. Only Cairn Done plus completed installed checks finish this work.
