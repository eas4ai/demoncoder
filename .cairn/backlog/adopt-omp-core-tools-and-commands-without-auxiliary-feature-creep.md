# Adopt OMP core tools and commands without auxiliary feature creep

Surfaced from: USABLE-004
Captured: 2026-09-06T20:21:32.213Z

Status: Proposed adoption inventory; no implementation commitment selected.

## Developer direction

Use Oh My Pi as the main harness reference. The developer considers its core a
strong fit but wants to avoid its accumulation of auxiliary features. Preserve
DemonCoder's Rust-first direction and reuse cohesive implementations and tests
where they fit. The small initial tool set was a foundation, not the finished
product's feature limit. The existing preference for OMP hash-anchored edits
remains part of this direction.

## Clarified core boundary

The developer explicitly includes the core coding harness, plugins, commands
related to coding and agents, diffs, statusline, chat queue, advisor model,
independent agent models and inter-agent communication. These capabilities are
central to the desired product. They are not auxiliary features to remove or
indefinitely defer in pursuit of a minimal four-tool loop.

## Proposed core

| Area | Candidate behavior | Adoption approach and dependency |
|---|---|---|
| Commands | Slash-command registry, help, completion, model/effort selection, settings, context and usage inspection | Adapt OMP's shared command metadata so dispatch, help and completions agree. Session commands depend on real session operations. |
| Reading and search | Line ranges, numbered/anchored excerpts, directory listing, native grep/glob with bounded results, structural code search | Inspect OMP pi-walker/pi-ast for Rust extraction; adapt result formats and cancellation to DemonCoder. Select grammar coverage deliberately. |
| Editing | Hash-anchored edits, stale-content rejection, readable diff previews | Port the focused OMP hashline format/applier with its tests; the package is TypeScript and depends on pi-natives/pi-utils, so it is not a drop-in Rust crate. |
| Terminal workflow | Multiline editing, prompt history, file mentions, command completion, rendered Markdown/code/diffs, collapsible tool output, useful statusline, structured questions and todos, distinct steering and follow-up queues with queue inspection/editing | Use OMP/pi behavior and test cases while integrating with the bounded DemonCoder transcript. The statusline should expose model, context, usage, Git and agent activity without unbounded background work. |
| Plugins | Discoverable/installable plugins contributing coding tools, commands, skills and lifecycle hooks | Inspect the OMP extension contract and representative plugins. Decide and test compatibility explicitly; a compiled Rust hook alone is not the requested plugin experience. |
| Context and sessions | Automatic scoped repository instructions, skills and prompt templates; saved sessions, resume, compaction, branching/checkpoints and export | Adapt pi/OMP session behavior. Preserve original history separately from model summaries. Fit persistence and accounting into verification-review-recovery. |
| Code intelligence | LSP diagnostics, definitions, references, symbols, rename and code actions | Adapt the request/response behavior around language-server processes. Edits must retain ordinary write and stale-content checks. |
| Execution | Background jobs with visible output/status, cancellation and cleanup; bounded long-running checks | Extend owned process lifecycle and shared budgets. A job must remain attributable and controllable after a turn finishes. |
| Coding evidence | Inspect diffs, run meaningful checks, review real changes, show unresolved findings | Keep model completion distinct from accepted work. Integrate the existing verification/review roadmap. |
| Agents and advisor | Focused subagents, visible work/status, independent agent and advisor model assignments, ongoing advisor feedback, inter-agent messaging, steering/cancellation and controlled change integration | Borrow OMP task/Agent Hub/advisor interactions. Messages retain identity and reach the intended active agent. Advisor work has its own context and visible usage, and shares the task's accounting/cancellation policy. |

Proposed command families: /help, /model, /settings, /context, /usage, /todo,
/new, /resume, /name, /session, /compact, /tree, /fork, /copy, /export,
/reload, /review and /agents. These are a proposed DemonCoder surface, not
commands implemented today. A command is added with its working behavior and
verification, rather than advertising a placeholder.

## Auxiliary features are separate choices

Voice/TTS, image generation, desktop control, collaboration relays, remote/mobile
hosting, marketplaces, multiple memory-service backends and autonomous research
are not part of the proposed core. They can be considered individually if a
real DemonCoder workflow calls for them.

Browser automation, debugger integration, persistent evaluation kernels, MCP
integration and editor protocols merit separate decisions. Some coding tasks
benefit substantially from them, but adopting OMP does not automatically select
all of them. Web search and readable URL/documentation results are strong coding
tool candidates and do not require importing a desktop-control subsystem.

## Reuse strategy

Recommended: use OMP as the behavioral baseline, extract cohesive Rust components
where practical, and port focused TypeScript components with their test cases.
Original pi supplies simpler session and command examples. The Rust pi reference
may supply focused Rust implementations; T3 Code is useful for Git/diff/review
interaction ideas. Source language alone does not establish a cheap extraction.

Alternatives considered: running OMP TypeScript behind a Bun helper could reuse
more code initially but adds a runtime and another process lifecycle. Forking the
whole reference carries its large feature and dependency surface forward. Neither
is selected by this proposal.

A core addition should help a developer or agent read, understand, change,
verify or manage coding work. It needs a clear owner, usable controls, bounded
resource behavior and a meaningful failure test. Broader useful features can be
optional without becoming permanent default startup cost.

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

## Suggested delivery structure

Define one coherent target experience covering the clarified core, then implement
it in verifiable slices. Do not let the slices redefine the finished product.

- Everyday coding surface: hash edits, ranged reads, search, commands, diffs,
  statusline, structured questions/todos and the steering/follow-up queue.
- Extensibility: plugin discovery/loading and coding tools, commands, skills and
  hooks, with an explicit OMP compatibility target.
- Session and agent foundations: durable context and compaction, shared accounting,
  independent model assignments and scoped instruction loading.
- Coordinated coding: advisor feedback, inter-agent communication, an agent roster
  with live inspection/control, verification/review and change integration.

The dependency order still needs source-level design. Advisor, plugins and agent
communication remain part of the target core throughout that work.

This ordering is a proposal for developer selection. The current completed
commitment is unchanged, and later product goals remain on the roadmap.

