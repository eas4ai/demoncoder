# Exclude private source before capture review and delegated Git export

Level: Judged
Decided by: Codex
Rests on: AUD-001 AUD-002 CODE-007 VERIFY-003 SUB-003 SUB-005
Would be wrong if: A protected source value reaches new durable snapshots reviewer requests child files or shared Git objects, or an ordinary source change is silently omitted

## Decision

Use one deterministic export policy for reserved runtime directories and credential filenames. Exclude private entries before opening or hashing contents, reapply the policy when formatting older snapshots, and prohibit those paths in delegated source trees. Show the excluded categories in captured scope and reviewer evidence. Continue rejecting declared connection/session storage inside task roots. This changes new exports; do not rewrite historical Git objects or session evidence.

## Realized by

76cc8c3b17e51488ef47309f91db31297efb5f9b Protect private source exports and restore installed Codex routing checks

`src/export_policy.rs` defines the shared exclusions and resolves declared credential aliases before workspace admission. Workspace capture and review use it before source export; delegated content validation refuses private paths. `tests/workflow_workspace.rs`, `tests/subagent_worktrees.rs` and `tests/audit_remediation.py` exercise synthetic private contents, historical snapshots, Git objects, and actual terminal/provider boundaries.
