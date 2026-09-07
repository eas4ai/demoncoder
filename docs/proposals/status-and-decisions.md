# Status and decisions in the terminal

Status: Proposed 2026-09-07. Not an implementation commitment.

Help the developer inspect progress and decide what to do next using existing
task and agent state. Source findings and limits are in docs/recon.md.

## Proposed behavior and proof

| Behavior for agreement | Falsifier | Proposed mechanism |
|---|---|---|
| Show actual active, waiting, held and ready agent counts alongside task verification, review and acceptance. | Counts disagree with retained state, a dropped notice leaves them wrong, or worker completion appears accepted. | Rendered-cell tests and production PTY with held children, queue pressure, cancellation and state refresh. |
| Show a selected assignment's objective, connection/model, owned changes, original checks, findings, response and judgment in readable sections with bounded paging. | Routine inspection requires decoding the entire JSON record, stale evidence looks current, pages silently omit evidence, or history changes. | Production inspection cases with failure/pass, stale snapshots, corrections, large records and narrow-screen navigation. |
| Explain waiting/held reasons and show applicable existing commands with their consequences. | Inapplicable actions lack a reason, agent messages authorize effects, or navigation executes a mutation. | Dependency, failed-review, exhausted-correction and recovery cases; inspect actual effects and refusals. |
| Preserve input, scroll position and cancellation during inspection. | Opening a view loses a draft, jumps history, delays cancellation or shows stale recovered state. | Held-provider PTY, resize, Unicode, queue saturation and restart tests. |

Illustrative view, not final layout:

```text
Task: improve parser       Verification failed · Review blocked
Agents: 1 active · 1 waiting · 1 ready for integration

Agent 2: parser tests
Waiting for: Agent 1 to be integrated
Next: inspect Agent 1's changes and checks

Agent 1: parser implementation
Checks passed · Advisor clear · Not integrated
Action: /agent-integrate 1 — applies validated changes to this workspace
```

## Existing owners and affected files

| Area | Source and contract |
|---|---|
| Terminal projection and evidence presentation | `src/terminal.rs:34`, `src/terminal.rs:336`, `src/terminal.rs:760`; `src/chat.rs`; `src/transcript.rs`; CHAT-002, USABLE-001, SWEEP-005. |
| Authoritative status refresh | `src/events.rs:247`; `src/subagents/manager.rs:143`; `src/workflow/mod.rs:129`. Live notices can be omitted; choose a bounded snapshot/refresh path during planning. |
| Developer controls | `src/subagents/session.rs:51`; `src/workflow/mod.rs:176`; SUB-004, SUB-005, VERIFY-001, ORCH-006. Reuse existing admission and authority gates. |
| Durable evidence | `src/subagents/state.rs:118`; `src/workflow/state.rs:28`; SUB-007, VERIFY-006, ORCH-007. Derive views from these records. |
| Verification | `tests/status_sweep.py`, `tests/assignable_subagents.py`, `tests/advanced_orchestration.py`, `tests/verification_workflow.py`, `scripts/check-sweep-status.sh`. Add rendered-screen assertions to existing production fixtures where practical. |

Cairn commitments belong to development tooling (docs/spec/overview.md). The
initial product view shows runtime tasks and assignments. Showing a repository's
Cairn commitment would be an additional integration to specify.

Provider adapters, confinement, model selection and scheduling keep their current
owners. General Markdown, completion, plugins, message editing and compaction
remain in the broader development-experience backlog.

## Priority and draft review

The roadmap already selects evidence-based improvement next
(docs/spec/roadmap.md; docs/commitments/advanced-orchestration.md). Decide whether
this UX slice precedes it. Approving the direction authorizes drafting its
requirements and falsifiers for agreement, not implementation by itself.

Draft review challenged dropped status events, stale evidence, completion versus
acceptance, mutation through navigation, recovery authority, bounded evidence,
narrow screens and draft retention. Proposed checks cover those cases. Exact
keybindings and countdown behavior remain design questions. No new check has
been built or run.

The footer defect, stale README allocation claim and three lint findings remain
recorded in docs/recon.md. Confirm the intended behavior before revising Agreed
text; runtime changes require selected scope.

