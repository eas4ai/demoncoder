# Remediate the end-to-end code audit

Status: Agreed 2026-09-08
Requirements: AUD-001, AUD-002, AUD-003, AUD-004, AUD-005, AUD-006, CODE-004, CODE-006, CODE-007, CODE-008, CONN-003, CONN-004, CONN-005, REL-003, REL-004, VERIFY-001, VERIFY-002, VERIFY-003, VERIFY-005, VERIFY-006, SUB-002, SUB-003, SUB-005, SUB-007, ORCH-004, ORCH-006, ORCH-007, REM-002, REM-004, LEARN-006, LEARN-007, SET-005, SET-007, SET-008

The developer selected the six numbered audit findings for remediation.
Close both private-source export paths first, make build and review scopes
practical and explicit, remove quadratic stream copying, and restore actual
installed Codex coverage without relaxing provider policy.

Done when the new falsifiers and named inherited requirements have current
passing evidence, the cumulative product regressions pass, an independent
review has no unresolved findings, and the installed release passes the
security, workflow and provider regressions. Preserve historical evidence;
do not rewrite existing Git history or delete prior private session records.
