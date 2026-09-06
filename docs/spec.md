# DemonCoder

Status: Working product specification. Implementation is not authorized by this draft.

The developer selected Cairn for development on 2026-09-06. The structured
[Cairn specification](spec/overview.md) now holds the first commitment's
draft requirements and falsifiers. This document remains the broader design
narrative and reference record; it does not override an Agreed requirement.

This document starts from the developer's stated goal: a TUI coding assistant with assignable subagents, advanced orchestration, and evidence-based self-improvement. It incorporates lessons from the source review of Suprnova Coder at `be3361a7a3eaa5081a33909ca62e28061b441c18`. That implementation is reference material, not DemonCoder's architecture or acceptance authority.

The product goal is established. Behaviors and design choices below are proposals for discussion until the developer accepts the specification. An unfinished implementation must remain described as unfinished.

## Product

DemonCoder is a terminal application for working with an agent on an existing codebase. The developer can watch its work, correct its direction, inspect changes, and delegate bounded work to visible subagents. The application can use evidence from its own operation to propose improvements to itself.

The first implementation milestone must deliver a usable terminal coding loop. Subagents, orchestration, and self-improvement remain required parts of the complete product; staging them must not turn them into an indefinite backlog.

Success means a developer can complete and continue real coding work through the terminal interface. A collection of schemas, storage APIs, or independently passing component tests does not establish that result.

## Ordinary coding loop

1. Open a repository and start or resume a coding session.
2. Enter a task. See assistant text, tool activity, and pending decisions while the agent works.
3. Submit a correction while work is running. The runtime applies it at the next safe tool boundary and shows that acknowledgement.
4. Queue an optional follow-up for after the current work stops. It remains distinct from a correction to current work.
5. Inspect the actual patch, verification results, and unresolved findings before accepting the outcome.
6. Continue in the same session with its conversation and resulting workspace state.

The editor remains responsive while models, tools, verification, or subagents run. Assistant output and tool results come from runtime events. A status message is not a replacement for a transcript.

Cancelling a turn stops its work without discarding the session. Closing the application preserves enough state to explain what completed, what was interrupted, and what requires a decision before continuing.

A second task such as “now extend that feature” must see the first task's code and relevant conversation. The application must not silently create an unrelated task from the repository's original commit. Existing uncommitted changes must be identified and preserved under an explicit workspace policy.

## Assignable roles and subagents

The developer can bind model responsibilities independently to supported providers and models. Selecting a role is separate from assigning a concrete subtask to an agent.

A subagent receives an explicit assignment containing:

- Its objective and expected result.
- Its selected role, provider, and model.
- The context and evidence it needs.
- Its permitted tools and owned files or explicit coordination boundary.
- Its resource allocation and completion criteria.

The terminal shows each subagent's assignment, status, activity, result, and usage. The developer can inspect it and cancel it. The parent can continue useful independent work while delegated work proceeds.

Each mutating subagent works in its own isolated workspace. The runtime integrates its changes into the parent workspace through an explicit operation. Conflicts become visible work; an agent cannot silently overwrite another agent's output.

Parent cancellation cancels active descendants. Delegated work consumes the parent's cumulative task allocation. A subagent cannot grant itself additional permissions or budget. Concurrency is bounded by configuration.

Messages preserve their sender, recipient, and assignment. Peer messages are labeled as agent evidence. They must never be rendered or persisted as human instructions. Delivery, waiting, cancellation, and restart must preserve message identity without treating an unrelated message as a reply.

## Supervision

The primary agent performs ordinary coding and decides when bounded delegation is useful. Plain code owns execution state, permissions, budgets, cancellation, verification status, and integration.

The earlier project's creator, task, smol, reviewer, advisor, and judge responsibilities are a candidate role design. The advisor's observation cadence and the critic/defender/judge resolution policy need an explicit decision in this specification before implementation. They must not be silently discarded, implemented from an old document, or assumed necessary for every task.

Any review mechanism receives the actual patch and relevant source, test results, and task requirements collected by the runtime. A worker's description of its work cannot substitute for that evidence. A tool-free reviewer must receive sufficient evidence before being asked for a verdict.

The correction path is the same for direct and decomposed work: record a finding, give it to the responsible agent, allow bounded correction, verify the correction, and obtain the required follow-up review. A blocked review cannot become success because the task took a different planning path.

## Verification and execution boundaries

The runtime distinguishes “the agent stopped,” “verification passed,” and “the requested work was accepted.” It shows the checks actually run and their limitations. Whitespace validation alone cannot establish that an arbitrary coding task is correct.

Verification commands execute code that the agent may have changed. They therefore use an explicit confinement and environment policy, just as ordinary tool execution does. Running in a worktree is not process isolation. Provider credentials and unrelated host state must not become available through a verification subprocess.

Pause and cancellation reach verification and subprocesses. Every phase receives the remaining cumulative deadline and resource allocation. A sequence of individually bounded operations cannot reset the task's budget.

Permission answers have explicit scope and survive restart. An answered request is not asked again solely because the application restarted. An unanswered request does not authorize execution. A permission mode cannot bypass invariant protections.

Recovery does not promise universal exactly-once execution of arbitrary tools. It distinguishes a known completed operation from an interrupted operation with an uncertain outcome and reconciles or asks before repeating a potentially non-idempotent action.

## Self-improvement

Self-improvement is an observable product workflow:

1. Actual operation produces a cited observation: a failure, correction, recurring friction, or escaped defect.
2. A defined detector or explicit annotation converts supporting observations into a structured deficiency.
3. Discovery proposes a bounded correction with its evidence, expected benefit, behavioral check, and risks.
4. The developer authorizes a correction task.
5. The ordinary coding, verification, review, and integration workflow handles that task.
6. Subsequent evidence records whether the correction addressed the deficiency.

The primary interface can inspect candidates and create a correction task with its motivating evidence attached. Discovery itself grants no execution authority.

Relevant lessons are retrieved and supplied to later work with their sources. Repository instructions are loaded according to a documented precedence and scope. Both mechanisms require tests through production task creation, not only prompt-assembly tests with manually supplied context.

The product does not reduce arbitrary engineering work to a universal score. Measurements remain specific to the behavior being evaluated.

## Architecture constraints

Each lifecycle has one runtime owner. DemonCoder is its own Rust application and selectively reuses components whose boundaries, dependencies, behavior, and reuse terms have been established. For an adopted component, preserve or deliberately replace its ownership model; do not add a competing copy of its turn state or a second scheduler for its children. The TUI issues commands and renders runtime events. Replaying retained events reproduces the relevant visible state, including interrupted and pending work.

Each durable fact has one declared authoritative representation. Other views are derived where practical. A separate database, daemon, service, or journal requires a concrete requirement that a simpler arrangement cannot meet. The specification does not prescribe a crate count or a storage format before those requirements are understood.

A dependency graph schedules work; agents do not directly mark themselves accepted or integrated. Provider integrations report capability limits honestly, including unavailable usage dimensions and unknown prices.

Reference projects justify specific borrowed mechanisms through inspected source paths and behavioral examples. Their overall architecture, terminology, and feature inventory are not requirements to copy.

## Small coding core and hooks

Developer direction: the basic pi loop with a small tool set and hooks is the desired foundation. Reuse cohesive, established pieces where they fit. Recommendation: implement a small Rust core preserving pi's basic loop semantics, with selectively adopted Rust components and tests where their dependencies are appropriate. A Rust adaptation is a new implementation to verify; it does not inherit reliability merely by resembling pi or copying part of a port.

The ordinary loop prepares context, asks the model, validates and executes permitted tool calls, records their results, and repeats until the model finishes or execution stops. Start with four coding tools: `read`, `write`, `edit`, and `bash`. Dedicated search or other tools can be added when a concrete workflow needs them. Four default tools are a starting surface, not a limit on required delegation capabilities or restricted-role tool sets.

The native core owns conversation progression, tool-call/result identity, cancellation, usage accounting, and explicit stopped or failed outcomes. The surrounding session runtime owns persistence and the task's acceptance policy. The TUI observes events and submits commands. Native subagents use the same coding core with their own assignment, context, tools, and allocation; a separate scheduler owns their coordination. An external agent backend owns its own session loop and exposes its tested controls through an adapter. The native core does not simultaneously advance that external session.

Start with a small typed hook interface for context preparation, request admission, tool admission, tool results, turn completion, and session lifecycle. Name the owner of each decision and specify hook order, errors, cancellation, and allowed effects. First-party Rust modules can use these interfaces; a general scripting runtime or compatibility with every pi extension is not a prerequisite.

Hook rules:

- Hooks may propose context changes, findings, or follow-up work. Runtime commands admit any resulting execution under the same permissions and cumulative allocation as ordinary work.
- Permission and budget checks run on the final request or tool arguments after permitted transformations and before execution. Changed arguments are validated again. A later hook cannot reverse an invariant denial.
- A failed or timed-out admission hook holds the affected execution with a visible reason. Observers cannot silently stall execution; their delivery and failure policy must be explicit.
- Preserve the original tool result, verification evidence, and usage record. A hook may derive a presentation or context summary, but cannot rewrite a failed check into a recorded success.
- Hooks cannot recursively launch untracked model calls or tasks. Advisor, review, compaction, and learning work has an owner, cancellation path, and accounted usage.

Delegation, supervision, and self-improvement use these interfaces and ordinary runtime commands. They remain required product capabilities. They do not create alternative versions of DemonCoder's native coding loop. External agent backends remain explicit adapters with one owner per session. Select product policies from the requirements rather than importing every feature of a reference application.

Before adding advanced orchestration, demonstrate the core through the TUI on a temporary repository: read a file, apply an edit, run a relevant check, correct a failed attempt, and continue the session. Negative cases include a denied tool that never executes, changed arguments that cannot bypass admission, interruption during a running tool, and a failing verification result that a hook cannot erase. Later slices add visible subagents and the full supervision and self-improvement workflows.

The intended improvements over a bare coding loop are visible and steerable delegation, consistent verification and correction, and evidence that helps later work. Whether DemonCoder performs better must be established on representative tasks; architecture and language choice alone are not evidence of improvement.

## Provider extensibility

The developer requested OpenAI subscription and API-key access, Anthropic
API-key access, and Claude subscription access through the Agent SDK or
headless CLI. The developer also wants a broader provider list supported
through adapters. The initial connection requirements are in
[connections.md](spec/connections.md).

Direct model providers feed the native loop. Codex app-server and Claude
Agent SDK/headless CLI are external agent backends. They share a connection
registry and visible session controls, but have different ownership and
capability contracts. An adapter cannot claim a control it cannot enforce.

Start with typed Rust adapter interfaces, registration, and configuration.
Additional providers can be registered without changing the coding loop or
terminal renderer. Separately installed adapters can follow when needed;
building a plugin marketplace or general scripting runtime is not part of
the first commitment. Authentication selection never silently changes the
account or billing method.

## Product foundation

Developer direction: build DemonCoder independently and copy selected useful code from reference projects with attribution, instead of maintaining a whole-project fork. Rust and reuse of an existing coding loop remain preferences. No comparative reliability or performance claim has been established.

Original pi's small loop and hook model are the primary behavioral reference. The local `reference/pi_agent_rust-main/` snapshot is one source of candidate Rust components. For each proposed component, record its source revision or snapshot identity, files, purpose, applicable license notices, required dependencies, and retained behavioral tests. Identify the smallest coherent component that satisfies a requirement before copying it. A copied file's imports, runtime assumptions, configuration, and storage expectations are part of that assessment. Do not assume a large application's agent loop can be extracted as one independent file.

No component has yet been selected or copied into DemonCoder's implementation. The coding loop, provider interfaces, tool interfaces, and session behavior are the first reuse candidates to assess. The TUI and orchestration design must satisfy this specification; the reference project's entire feature inventory is not inherited scope.

This Rust implementation develops independently: its current README explicitly ends strict legacy-pi drop-in compatibility as a product goal and names Oh My Pi as the closer product reference. DemonCoder must validate the behavior it needs directly. Historical parity artifacts cannot establish current compatibility. The SDK guide still contains older certification and CI language that differs from the current README; resolve such claims against a pinned revision's source and actual checks before relying on them.

Selective copying makes DemonCoder responsible for maintaining and testing the adopted code. Relevant upstream fixes must be reviewed and applied deliberately; they will not arrive automatically through dependency updates or a fork merge. Prefer cohesive reusable components and focused adaptations over copying a broad subsystem and attempting to remove unwanted behavior afterward.

The terminal toolkit is still a design choice. The previously discussed tuie fork and the reference project's toolkit are candidates, not commitments. A provider's authenticated CLI transport remains a separate choice from the application's coding engine.

### Reuse licensing and attribution

The developer states that they do not work for Anthropic or OpenAI. Use of an AI coding tool does not by itself establish that the developer acts on the provider's behalf. Do not assume that relationship or require separate permission on that assumption.

The snapshot's [LICENSE](../reference/pi_agent_rust-main/LICENSE) is labeled MIT with an OpenAI/Anthropic rider. The [published license](https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/LICENSE) also contains that rider. Record the actual terms with each adopted component, preserve applicable copyright and license notices including the rider, and provide attribution. Do not relabel the source as standard MIT or remove its conditions. This document records provenance and the developer's clarification; it does not determine the rider's legal applicability or enforceability.

### Local snapshot size

A read-only inventory of `reference/pi_agent_rust-main/` found 225 Rust files under `src/` totaling 553,612 physical lines, and 395 Rust files under `tests/` totaling 401,098 physical lines. Counts include comments, blank lines, and any inline tests; they are not production-only code counts. The snapshot also contains fixtures, documentation, and a bundled TypeScript pi reference. This establishes size, not quality or extraction cost.

For comparison, the local original-pi `pi-0.84.2` reference has 796 physical lines in `packages/agent/src/agent-loop.ts`. Its complete agent source package has 12,611 lines, including harness and testing support; provider source has 23,123, coding-application source 58,914, and TUI source 16,733. These counts cover `.ts` and `.tsx` files under each package's `src/`, including comments and blank lines. The loop file is not a standalone engine. Evaluate the loop and its required dependencies separately from adopting an entire application; neither the Rust port's total size nor the TypeScript loop's single-file size establishes that boundary.

### Component assessment and gap specification

Assess candidate components against the product contract before planning their adoption. For each requirement, record the source path, dependencies, the behavioral check, its observed result, and any missing behavior. Classify it as demonstrated, partial, missing, or unverified. Retest adopted behavior through DemonCoder's actual execution path; the reference application's behavior alone does not prove the extracted component works in its new context. Check:

- Open DemonCoder's TUI, start a session, and stream assistant and tool events while the editor stays responsive and preserves call, turn, session, and sender identity.
- Continue two coding tasks in the same session and workspace.
- Demonstrate steering, follow-up queue behavior, and cancellation under active model and tool execution. Verify this draft's next-safe-tool-boundary behavior directly; the existence of queue or abort APIs does not prove delivery timing or subprocess cancellation.
- Show that a tool can be denied before execution and that ordinary tools and verification use the intended confinement and environment policy. The SDK's tool-start notification returns no approval decision; it is not itself a permission gate. Evaluate the custom tool registry or another supported enforcement boundary.
- Enforce a shared allocation across parent and child work, including hidden auxiliary calls such as compaction. Merely reading final usage counters does not prove admission control.
- Launch one independently configured child in an isolated worktree; cancel it and integrate its result without granting it parent or user authority.
- Recover a session with interrupted work and pending decisions without blindly repeating an uncertain mutation.

For a gap, identify the adopted or new component responsible and its behavioral acceptance check. If extraction requires importing a broad subsystem or rewriting its architecture, compare that cost with another source or a focused implementation before committing to it. Do not weaken the requirement merely to preserve a reuse choice.

### Evidence inspected for the foundation choice

- [Original pi README](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/README.md): four default tools and an extensible minimal harness. [Extension documentation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md): context, lifecycle, tool-blocking, and result hooks. The local `pi-0.84.2` documentation explicitly says tool-argument mutations are not revalidated; final-argument validation above is a proposed DemonCoder contract, not a claim about pi's behavior or a demonstrated exploit.
- [Rust implementation product direction](https://github.com/Dicklesworthstone/pi_agent_rust#current-product-direction): independent development with Oh My Pi as the closer reference; strict legacy-pi compatibility is no longer the product goal.
- [Rust SDK source](https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/src/sdk.rs) and [embedding example](https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/examples/basic_sdk.rs): supported module, session configuration, event subscriptions, cancellation exports, and custom tool integration. Source and example were inspected; the example was not executed.
- [Oh My Pi agent loop](https://github.com/can1357/oh-my-pi/blob/main/packages/agent/src/agent-loop.ts): `agentLoop` and `agentLoopContinue` call the TypeScript loop implementation. This is direct source evidence that the agent loop has not been replaced by Rust.
- [Oh My Pi Rust addon manifest](https://github.com/can1357/oh-my-pi/blob/main/crates/pi-natives/Cargo.toml) and [workspace manifest](https://github.com/can1357/oh-my-pi/blob/main/Cargo.toml): Rust native components coexist with that TypeScript loop.
- [Oh My Pi feature documentation](https://github.com/can1357/oh-my-pi#readme): describes subagent workspaces, Agent Hub, steering, and an advisor model. These are candidates for source inspection and behavioral verification, not established DemonCoder capabilities. No upstream commit-history comparison has run; release activity does not establish a small fork or inexpensive synchronization.
- [Upstream pi RPC documentation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc.md): headless protocol, events, steering, follow-ups, abort, and session commands. The local `pi-0.84.2` reference documentation was also inspected.
- [Independent Rust implementation SDK guide](https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/docs/sdk.md): embedding interface and supported API surface; its older compatibility and CI claims differ from the current README and are not adoption evidence.
- [Independent Rust implementation manifest](https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/Cargo.toml): its own async runtime, storage, and JavaScript-extension dependencies. The manifest was inspected; the implementation was not audited or benchmarked.

These are documentation, manifest, limited source, and file-inventory observations. No code has been copied into DemonCoder's implementation, and no application build, behavioral test, or performance comparison has run for DemonCoder.

## Delivery sequence

| Slice | Required demonstration |
|---|---|
| Terminal coding loop | A task changes a temporary repository; text and tools stream; steering and cancellation work; a second task continues from the result. |
| Verification, review, and recovery | A meaningful failing check blocks completion; correction is possible; review receives the patch; interruption preserves pending decisions and usage without unsafe replay. |
| Assignable subagent | A parent delegates bounded work while continuing independently; the child is visible and cancellable; its changes integrate or produce an explicit conflict. |
| Advanced orchestration | Independent assignments run within bounds; dependent work waits; failures and supervision follow the accepted policy without changing authority labels or bypassing verification. |
| Self-improvement | A production-generated deficiency leads to a cited candidate, an authorized correction, and evidence that the correction works; a later task receives the relevant lesson. |

Every slice includes a negative case that demonstrates what prevents incorrect success. Automated repository exercises use temporary repositories. Testing against a developer repository requires that repository to be explicitly selected.

## Decisions needed before implementation planning

The direction is an independent Rust application with a small pi-style coding core, four initial coding tools, typed hooks, and selective code reuse. Select cohesive components with understood dependencies and record their provenance and license terms. Then settle the terminal toolkit, session workspace and integration policy, initial provider support, and supervision policy. The implementation plan should name what will be adopted, what will be adapted, what must be written, and how each part will be verified. This remains a specification draft; implementation has not started.
