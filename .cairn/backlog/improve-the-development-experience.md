# Improve the development experience

Surfaced from: USABLE-004
Captured: 2026-09-06T20:21:32.213Z

Status: Proposed improvement inventory; no implementation commitment selected.

## Goal

The goal is to improve the development experience: help developers understand
code, make reliable changes, direct and coordinate agents, review results, and
continue work with less friction.

DemonCoder should make the state of the work clear and give the developer useful
controls throughout a coding session. Tools, plugins, commands, diffs, the
statusline, chat queues, advisor feedback, model assignments and inter-agent
communication contribute to that experience. Their value comes from how well
they support real development work.

## Desired improvements

| Development need | Capabilities to support it |
|---|---|
| Find and understand relevant code | Ranged and anchored reads, directory listing, native text/path search, structural search and language-server navigation. |
| Make accurate changes and inspect their effects | Hash-anchored edits, stale-content rejection, readable diff previews, diagnostics, rename and code actions. |
| Direct work without interrupting the flow | Discoverable coding and agent commands, completion, multiline editing, prompt history, file mentions, structured questions and todos. |
| See what is happening | A useful statusline showing model, context, usage, Git and agent activity; readable Markdown/code/diffs; collapsible tool output; visible job status. |
| Manage messages while work continues | Separate steering and follow-up queues, with ways to inspect, edit and withdraw queued messages. |
| Adapt the harness to the project | Discoverable/installable plugins contributing coding tools, commands, skills and lifecycle hooks; scoped repository instructions and prompt templates. |
| Continue long or interrupted work | Saved sessions, resume, compaction, branching/checkpoints, export and cumulative resource accounting. |
| Use the right models and coordinate their work | Independent main-agent, advisor and worker model assignments; advisor feedback; inter-agent communication; live inspection, steering and cancellation. |
| Establish confidence in the result | Meaningful checks, review of actual changes, visible unresolved findings and controlled integration of agent changes. |

These improvements form a coherent target experience. Delivering them in stages
must preserve the role of plugins, advisor feedback and agent coordination in
that target.

Candidate commands include /help, /model, /settings, /context, /usage, /todo,
/new, /resume, /name, /session, /compact, /tree, /fork, /copy, /export,
/reload, /review and /agents. Each command should expose a working capability
with clear behavior and verification. Exact names remain a design choice.

## Choosing additions

Evaluate each addition by the development problem it solves, how often it helps,
and the complexity it introduces for users and maintainers. Keep the default
experience understandable, responsive and dependable as capabilities grow.

Browser automation, debugger integration, persistent evaluation kernels, MCP and
editor integration should be evaluated against concrete coding workflows. Web
search and readable documentation results are useful candidates for research
within those workflows. Features such as voice, media generation, desktop
control and remote collaboration can be considered when a demonstrated need
justifies them.

## Evidence of improvement

Verify complete development workflows: finding code, applying a correct edit,
inspecting its diff, correcting a failed check, steering queued work, resuming a
session and coordinating agents. Retain failures and limitations alongside
successful results. Useful controls, clear ownership, responsive operation and
reliable outcomes are the acceptance criteria to develop for each slice.

## Delivery planning

Group implementation around observable improvements to everyday coding,
extensibility, session continuity and coordinated agent work. Determine the
dependency order and specify meaningful acceptance checks before selecting an
implementation commitment.
