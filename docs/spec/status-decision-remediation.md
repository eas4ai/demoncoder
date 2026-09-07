# Status and decision remediation

Status: Agreed 2026-09-07
Prefix: REM

The developer classified this slice as prerequisite remediation and confirmed
the proposed behavior and proof in docs/proposals/status-and-decisions.md.
Complete it before starting evidence-based-improvement. Reuse the existing
terminal, task owner, delegation owner and private runtime record.

[REM-001] The terminal MUST show active, waiting, held and ready agent counts from authoritative retained state alongside distinct task work, verification, review and acceptance states.
The view MUST recover from omitted advisory notices without inventing a zero or successful outcome.
Falsifier: Counts disagree with retained state, a dropped notice leaves the view wrong, unavailable state appears measured, or worker completion appears accepted.
Mechanism: status-decision-remediation; rendered-cell and production PTY checks with held children, state changes, queue pressure and cancellation.

[REM-002] The developer MUST be able to inspect task and agent objectives, connection/model, owned changes, original checks, findings, worker responses and judgments in labeled sections with bounded paging.
Inspection MUST preserve original evidence and identify its snapshot and freshness limits.
Falsifier: Routine inspection requires decoding the full record, pages silently omit evidence, stale or unchecked evidence appears current, or inspection rewrites history.
Mechanism: status-decision-remediation; production inspection and formatter cases with failure/pass, changed snapshots, multiple correction rounds, large evidence, Unicode and narrow layouts.

[REM-003] Inspection MUST explain waiting or held work and show applicable existing developer commands with their consequences or refusal reasons.
Navigation MUST NOT execute a mutation or treat agent messages as developer authority.
Falsifier: A blocked action lacks a reason, navigation changes files or task state, or a role message authorizes acceptance or integration.
Mechanism: status-decision-remediation; dependency, failed-review, exhausted-correction and interrupted-recovery cases with actual effects and command refusals.

[REM-004] The terminal MUST preserve the prompt draft, conversation scroll position and responsive cancellation while inspection is opened, navigated, resized and closed during work.
Inspection retention and refresh work MUST remain bounded.
Falsifier: Inspection loses a draft or history anchor, blocks input or cancellation on slow state access, retains unbounded pages, or restores stale inspection state after restart.
Mechanism: status-decision-remediation; held-provider PTY and deterministic view tests for Unicode, navigation, resize, refresh contention and recovery.

[REM-005] Documentation MUST describe the delivered status, inspection controls and actual allocation guarantees accurately.
The specification MUST pass the installed specification lint without weakening existing obligations.
Falsifier: Documentation retains the false no-deadline claim, misstates inspection behavior, the three recorded compound obligations remain, or a correction removes an acceptance, recovery or cancellation requirement.
Mechanism: status-decision-remediation; installed spec lint, focused documentation checks and manual comparison of revised obligations.

Views are derived, not another durable store. Recorded check/review success is
distinct from a new check of files; expose that limit wherever presenting saved
results. Acceptance and integration retain their existing current-file gates.
General Markdown, plugins, prompt-history features, compaction and a Cairn runtime
integration are outside this remediation.
