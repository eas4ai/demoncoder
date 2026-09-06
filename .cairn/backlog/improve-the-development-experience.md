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

Oh My Pi is a primary reference for useful behavior and reusable components.
Original pi, the Rust pi reference and T3 Code also provide ideas and source to
evaluate. Select and adapt those contributions according to DemonCoder's goals,
while preserving its Rust-first direction.

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

## Reuse approach

Inspect the reference implementation and its tests before deciding whether to
extract, adapt or port it. Preserve source identity, attribution and relevant
behavioral tests. Verify the resulting behavior through DemonCoder's ordinary
session and tool paths.

Initial candidates include:

- OMP's command registry, which connects dispatch, aliases, help and completion.
- OMP's hashline editing format and applier. Its package is TypeScript with
  pi-natives/pi-utils dependencies, so Rust integration needs a deliberate port
  or bridge. Hash-anchored editing is an existing developer preference.
- OMP's Rust pi-walker and pi-ast components for discovery and structural search,
  subject to dependency and grammar-coverage review.
- pi/OMP session, queue and agent interactions, adapted to DemonCoder's lifecycle,
  persistence and accounting contracts.
- Reference plugin contracts and representative plugins, with an explicit,
  tested compatibility target.
- Focused Rust reference implementations and T3 Code's Git/diff/review workflows.

Direct Rust reuse, focused TypeScript ports and a runtime bridge have different
costs. Choose per component after examining its dependencies and required
compatibility. A feature's presence in a reference is evidence to investigate;
its usefulness and integration quality determine whether it belongs here.

## Evidence of improvement

Verify complete development workflows: finding code, applying a correct edit,
inspecting its diff, correcting a failed check, steering queued work, resuming a
session and coordinating agents. Retain failures and limitations alongside
successful results. Useful controls, clear ownership, responsive operation and
reliable outcomes are the acceptance criteria to develop for each slice.

## Inspected source evidence

Local snapshot: /home/shawn/workspace2/oh-my-pi-18.1.2.

- packages/coding-agent/src/tools/builtin-names.ts: actual built-in tool IDs.
- packages/coding-agent/src/tools/index.ts: registration imports and session dependencies.
- packages/coding-agent/src/slash-commands/builtin-registry.ts: shared metadata,
  aliases, completion/help materialization and execution dispatch.
- packages/coding-agent/src/slash-commands/builtin-{session,lifecycle,control}.ts:
  inspected portions include todo/session management, new/fresh/clear and pause/quit.
- packages/hashline/package.json: TypeScript entry point and package dependencies.
- crates/pi-walker/Cargo.toml and crates/pi-ast/Cargo.toml: Rust component dependencies.
- README.md: broader feature inventory, used to find candidates rather than prove
  runtime or performance claims.

Additional local references:

- reference/pi_agent_rust-main/legacy_pi_mono_code/pi-mono/packages/coding-agent/README.md:
  session commands, branching, compaction, queue behavior and keyboard controls.
- reference/pi_agent_rust-main/src/context_files.rs: scoped foreign-rule parsing
  candidate and provenance; native AGENTS.md/CLAUDE.md loading belongs elsewhere.
- reference/pi_agent_rust-main/src/ast_tools.rs: staged structural edit contracts
  and dependencies on the reference's own tool/model types.
- reference/t3code-0.0.38/docs/user/source-control.md: Git and review interaction ideas.

This is an inventory and initial dependency assessment. The reference suites were
not executed, no upstream performance claims were adopted, and no source has been
copied by this proposal. Before extraction, identify the exact source revision or
content hash, license/attribution, transitive dependencies and retained tests.

## Delivery planning

Group implementation around observable improvements to everyday coding,
extensibility, session continuity and coordinated agent work. Determine the
dependency order through source-level design and specify meaningful acceptance
checks before selecting an implementation commitment.
