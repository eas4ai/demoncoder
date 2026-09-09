# Protect relative credential roots in launch and workspace directories

Level: Judged
Decided by: Codex
Supersedes: admit-only-owned-child-source-inside-the-session-container
Cause: the stated condition occurred
Rests on: AUD-001 AUD-002 CODE-007 VERIFY-003 SUB-003 SUB-005
Would be wrong if: A relative credential declaration reaches retained source, reviewer requests, child files or Git objects, or prior unsafe snapshots remain reviewable
History: Fixed-name filtering missed relocated roots; shared root rejection repaired absolute declarations while requiring an owned-child exception. Fresh independent review now reproduced relative declarations resolving differently in the launcher and backend. Preserve both execution-directory interpretations without changing provider configuration or tool authority.

## Decision

Retain launch-directory resolution of private roots and also resolve relative declarations against the selected workspace, matching inherited backend execution. Use the same root collector for developer access and source export, preserve the validated child allowance only for the default session store, and increment export-policy provenance so unsafe older baselines require a new task. Cover absolute, relative and parent-relative declarations with process-isolated capture, delegation and installed terminal checks.

## Realized by

(none yet: recorded, not built)
