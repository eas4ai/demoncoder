# DemonCoder: the developer harness

Status: Proposed feature specification — for developer review.
Baseline: DemonCoder 0.1.2, implementation commit bb1324c; completed chat-presentation review at e0769be.

This draft turns the [development-experience inventory](../../.cairn/backlog/improve-the-development-experience.md)
and [product direction](../spec.md) into observable features and staged delivery.
It does not change an Agreed requirement, select a Cairn commitment, or authorize
implementation. After agreement, the selected milestone's requirements and
falsifiers will enter `docs/spec/` and its commitment will enter the roadmap.

## 1. Product outcome

A developer should be able to open an existing project, understand relevant code,
direct a change, watch and correct the work, inspect the patch and its evidence,
and continue later without reconstructing what happened. For larger work, the
developer can assign models and bounded work to visible agents, then inspect how
their results were checked and integrated.

The ordinary workflow stays small:

1. Open or resume a project session.
2. Describe the intended result and any constraints.
3. Let the agent investigate, propose or maintain a plan, and work.
4. Ask questions, steer, queue follow-ups, or pause without losing the work.
5. Inspect changes, checks, findings, and remaining uncertainty.
6. Accept the result or request a correction; continue in the same project.

The harness owns execution, persistence, permissions, cancellation, resource
accounting, verification status, and integration. Models propose actions and
interpret evidence. A model saying “done” does not establish verified success.

### Current foundation

Version 0.1.2 provides the continuing terminal conversation, four initial
connections, native read/write/edit/Bash tools, typed tool hooks, project trust,
steering, cancellation, optional Oracle screening for explicit host execution,
usage reporting, bounded scrollback, activity headings, compact/full output,
code colors, and a scrollbar. These are foundations to retain, not features to
reimplement under a second coding loop.

Saved application sessions, general Markdown rendering, an extensible command
surface, native code search, hash-anchored editing, task acceptance, visible
subagents, and installable extensions are delivery work in this proposal.
The existing audit log is not a session-recovery implementation.

## 2. Delivery strategy

Three approaches were considered:

| Approach | Benefit | Cost |
|---|---|---|
| **Staged developer workflows — recommended** | Deliver dependable solo work first, then project extensions and coordinated agents. Every milestone is useful through the terminal. | Delegation arrives after the persistence and acceptance boundaries it needs. |
| Delegation first | Makes parallel agents visible quickly. | Either repeats unfinished persistence/review work in each agent or initially loses their work and evidence on restart. |
| One large harness release | Delivers the full inventory together. | Delays feedback and makes failures difficult to isolate across many new subsystems. |

The following sequence is proposed, not yet selected. Plugins, advisor feedback,
coordination, and self-improvement have named milestones and completion cases;
they are not removed from the product by staging delivery.

| Milestone | Developer-visible result | Feature IDs | Prerequisite |
|---|---|---|---|
| M1 — Dependable everyday coding | Useful editor and commands; readable results; native search and safer edits; the four reported reliability issues resolved or disproved. | REL-01–04, UX-01–04, TOOL-01–03 | Current release |
| M2 — Work that survives and can be accepted | Resume sessions, manage queued work and jobs, inspect context, compact deliberately, run required checks, review actual changes, and accept or correct work. | UX-05–07, SESSION-01–06, CONTEXT-01–04, TASK-01–05 | M1 |
| M3 — Project-aware extensions | Install and manage project skills, commands, tools and hooks; use language services and an MCP bridge through declared permissions. | EXT-01–04, TOOL-04 | M2 |
| M4 — Visible, assignable agents | Independently configured roles, advisor feedback, bounded child work, messages, cancellation and reviewed integration. | AGENT-01–06 | M2; extension entry points from M3 |
| M5 — Coordinated work and improvement | Dependency-aware assignments, bounded dispute resolution, cited improvement candidates and useful retained lessons. | ORCH-01–03, LEARN-01–03 | M4 |

M1 and M2 together constitute the proposed **reliable solo harness** release.
M4 constitutes the first **delegating harness** release. A milestone may require
several Cairn commitments, each ending in a complete observable workflow.

## 3. Reliability required before expanding the harness

The supplied review screenshot reports four issues. Their current applicability
must be checked; these entries do not treat a screenshot as a verified exploit.

| ID | Required behavior | Acceptance case and falsifier |
|---|---|---|
| REL-01 | Confined commands cannot use host control sockets to bypass the intended write boundary. Intentionally exposed services require a declared policy. | A private, harmless fixture service records connections. Default confined Bash cannot contact its host socket; an explicitly supported service remains usable under its declared policy. A read-only mount alone is not a passing check. Never probe Docker or another privileged host service. |
| REL-02 | Saturated command and event queues cannot freeze typing, cancellation or quit. Rejected submissions keep the developer's text and show why they were not accepted. | Hold the consumer and saturate both queues. The editor continues to acknowledge input, quit works, and cancellation reaches owned work within the existing grace period. Waiting for channel capacity inside the UI loop fails this case. |
| REL-03 | Streaming tool arguments have a documented byte bound enforced while accumulating data. Oversized arguments stop the response without admitting that tool. | A controlled Anthropic stream exceeds the bound without ending its block. Observe bounded retained data, a visible error and no tool effect before the stream ends. Checking only the parsed final arguments fails. |
| REL-04 | Streamed UTF-8 preserves valid characters split between reads, with separate state for stdout and stderr. Incomplete final bytes have a defined replacement behavior. | Split multibyte characters at every byte boundary, interleave both streams, and compare visible and retained output. Replacement characters in an otherwise valid stream fail. |

These repairs preserve ordinary developer networking and tool availability.
They do not silently replace the existing default-access or explicit `--yolo`
contracts. Any necessary change to those contracts is a separate decision.

## 4. Interaction and work visibility

| ID | Required behavior | Acceptance case and falsifier |
|---|---|---|
| UX-01 | The editor supports cursor movement, word movement/deletion, multiline composition, paste, current-session prompt history and retention of an unsent draft after rejected submission. Submission has a documented binding distinct from inserting a newline. | Edit the middle of a Unicode prompt, paste several lines, recall a prior prompt and resize while a tool streams. Losing text, accidental submission or blocking input fails. |
| UX-02 | A discoverable command menu exposes implemented actions, descriptions, argument completion and errors. File mentions resolve selected project paths and show what context will be attached. | Find an action from `/help`, complete a path containing spaces, and cancel before submission. An unknown command must not accidentally become a model instruction; a file mention must not silently attach unrelated files. |
| UX-03 | Assistant prose renders headings, emphasis, lists, inline code, code fences, tables and links. Tool output retains a literal view. The developer can inspect or copy original text and a link's destination. | Stream incomplete Markdown, display the supplied review-style message, and inspect a file-and-line link. Rendering must preserve accessible content, bounds and anchors; output cannot inject terminal controls or launch a link by itself. |
| UX-04 | The status area exposes active connection/model/effort, current work state, reported usage and Git state without obscuring the editor. Later milestones add task phase, context estimates and agent/job activity as those capabilities arrive. | Start a session with an explicit model, produce unknown usage and modify the worktree externally. Stale values presented as current, estimates presented as exact, or missing cancellation controls fail. Narrow terminals prioritize work state and controls. |
| UX-05 | Steering and follow-up messages are distinct, visible queues. Pending messages can be inspected, edited or withdrawn; the runtime acknowledges acceptance and application. | Queue both kinds during a held tool, withdraw one and restart with the other pending. Steering reaches the next safe boundary; a follow-up waits for a terminal task state. A follow-up after failure/cancellation remains held for explicit continuation rather than starting unexpectedly. |
| UX-06 | Questions, permission requests, plans and todos are typed work items. Each shows its owner and state; questions support a free-text answer. | Deliver an answer while output streams and update a todo. A stale answer must not resolve another question, and an agent cannot mark a required check passed by changing a todo. |
| UX-07 | Background commands are owned jobs with inspectable output, exit status, input where supported, and cancellation. They use the ordinary tool boundary and task allocation. | Start a development-server fixture, keep using the editor, inspect its output and stop it. Parent cancellation stops owned jobs. Recovery identifies stopped or uncertain jobs and does not silently restart commands or leave an untracked process. |

Proposed commands: `/help`, `/model`, `/settings`, `/context`, `/usage`, `/todo`,
`/new`, `/resume`, `/name`, `/session`, `/compact`, `/tree`, `/fork`, `/copy`,
`/export`, `/reload`, `/review`, `/agents`, `/jobs`, `/queue`, `/pause`, and `/continue`.
They appear as usable commands only when their backing feature exists.
Command aliases and key bindings are configuration, not separate workflows.

## 5. Code discovery and precise changes

| ID | Required behavior | Acceptance case and falsifier |
|---|---|---|
| TOOL-01 | Reads support ranges and stable source references. Directory listing and native path/text search report bounded results, truncation and continuation information. Ignore rules and excluded paths are explicit. | Find a symbol in a large temporary project, open the returned range and continue truncated results. Missing a result without a truncation indication or traversing a protected path fails. |
| TOOL-02 | Hash-anchored edits detect stale content and apply an unambiguous intended change. The tool reports the resulting patch or a specific conflict. Multi-hunk edits to one file are all-or-nothing; multi-file operations validate every input before the first write and report any interrupted partial application. | Read anchors, modify the file independently, then submit the old edit: no write. Re-read and apply the corrected edit successfully. Test repeated lines, Unicode, moved text and partial-write recovery. A hash label alone cannot establish that content is current. |
| TOOL-03 | The developer can inspect task changes by file and hunk, distinguish pre-existing changes, and navigate from a result or diagnostic to its source. | Start with a dirty worktree, make a task change, then make an external change. The view attributes only known task changes and identifies conflicts. Discard/revert operations act only on explicitly selected changes. |
| TOOL-04 | Optional language-service integration supports diagnostics, definition/references, rename and code actions with per-project capability reporting. | Run a fixture language server, navigate a symbol and preview a multi-file rename. A stale document version rejects the edit; server absence gives a clear fallback to ordinary tools. Unsupported languages are not shown as supported. |

Structural search and language servers complement file tools. They do not become
mandatory prerequisites for opening a repository or using the coding loop.

The local OMP 18.1.10 reference was inspected at
`crates/pi-edit/prompts/hashline.md`, `crates/pi-edit/tests/hashline_patcher.rs`
and `packages/coding-agent/src/tools/hashline-format.ts`. It uses session snapshot
tags, numbered visible lines and explicit stale/no-op diagnostics. Those are
behavioral references; this draft does not adopt its grammar or short hash format,
and its tests were not executed as evidence for DemonCoder.

## 6. Sessions, recovery and context

| ID | Required behavior | Acceptance case and falsifier |
|---|---|---|
| SESSION-01 | Sessions are saved by default in private application storage and can be listed, named and resumed for the correct project. Conversation, editor drafts/history, task identity, settings, queued work and pending decisions survive restart. | Close and reopen after two related turns. Continue the same task and workspace. Attaching to an unrelated backend identity, resetting allocation or silently starting a new session fails. |
| SESSION-02 | Recovery distinguishes completed, interrupted and uncertain operations. It reconciles available evidence before offering to continue; uncertain mutations are never blindly replayed. | Interrupt before admission, during a harmless mutation, and after its effect but before result publication. Show the corresponding state and avoid duplicate execution. Truncated or corrupt records produce an actionable recovery state, not invented success. |
| SESSION-03 | Checkpoints and conversation branches record their parent and workspace relationship. Forking a conversation does not silently restore files; restoring files is a separate explicit action. | Fork before a correction and compare both histories. Restore a selected task checkpoint with unrelated dirty files present. Losing those files or claiming the conversation fork reverted the worktree fails. |
| SESSION-04 | Export produces a readable transcript and machine-readable evidence with identities and explicit omissions. Private connection credentials are excluded; sensitive project content is not assumed safe to publish. | Export a session containing configured synthetic credentials and a deliberately omitted artifact. Credentials must be absent and the omission visible. Export remains a local file action; publishing requires its own instruction. |
| SESSION-05 | Storage retention is inspectable and configurable. Deleting a session explains its scope and does not delete project files or active work. Display retention, durable history and provider context are separate limits. | Expire display rows, resume from durable history, then remove one inactive session. Confusing a compact display with compaction or deleting another session's data fails. |
| SESSION-06 | Pause stops admission and interrupts owned execution through the existing cancellation path, preserving the task for explicit continuation. The UI reports paused only after work stops; uncertain effects are reconciled before resuming. | Pause during a provider request and a harmless tool, restart, then continue. No pending work runs while paused, and continuation neither loses constraints nor blindly repeats an uncertain mutation. Cancelling a task remains a distinct terminal outcome. |
| CONTEXT-01 | Project instructions and skills load through the production context path with visible source, scope and precedence. Repository text and retrieved material cannot create developer authority. | Use nested instructions and conflicting untrusted text. Show the effective sources and ensure instructions outside their scope do not govern the task. Automatic project discovery must not execute an extension. |
| CONTEXT-02 | `/context` explains what the next request will contain: conversation, instructions, attachments, tool results, summaries and relevant lessons. Sizes may be estimates, clearly labeled. | Select a file mention and then remove it. The next captured provider request matches the displayed selection and exclusions. A hidden context source or unsupported exact token claim fails. |
| CONTEXT-03 | Compaction can be requested and may be offered before the model's context limit. It preserves task constraints, unresolved findings, pending decisions and references to original evidence. The summary never replaces original receipts. | Force compaction between a failed check and its correction, then continue. Losing the failure, treating a summary as a human instruction, or making an unaccounted model call fails. |
| CONTEXT-04 | Requests receive only the context selected for their purpose, under the same access boundary. External-backend limitations are reported. This applies to main, review and compaction requests in M2 and to advisor/worker requests when M4 adds them. | Give a task and its reviewer different permitted context and inspect their actual input. Repeat for two child assignments in M4. Unrelated private context or a claimed context control that an adapter cannot enforce fails. |

Proposed instruction precedence: runtime invariants and explicit developer
directions govern; user-level defaults apply next; project instructions apply
within their scope, with more specific project instructions refining broader
ones. Skills, retrieved material, tool results and agent messages remain labeled
sources and cannot grant new permissions. Conflicts with an explicit developer
direction are surfaced rather than silently overridden by a repository file.

Answered permission decisions retain their original scope across restart;
resuming alone neither asks them again nor broadens them. Unanswered requests
grant nothing. New or changed actions still pass final admission, and an explicit
revocation prevents later use of the old grant. SESSION-01 recovery cases include
answered, unanswered and revoked permissions.

## 7. Task verification, review and acceptance

| ID | Required behavior | Acceptance case and falsifier |
|---|---|---|
| TASK-01 | A task records the requested outcome, constraints, plan, workspace baseline and acceptance criteria. Plans can change with a visible reason. Casual questions need not create a heavyweight change plan. | Start an investigation and a code change, amend the latter's scope, and inspect their records. A rewritten plan cannot erase a constraint or an unresolved finding. |
| TASK-02 | Verification runs named checks against identified workspace content, recording command, environment policy, exit, output, timing and limitations. A required failing or stale check blocks verified completion. | Run a failing test, correct it and rerun it; then change an input. The old pass becomes stale. A zero exit from an unrelated command cannot substitute for the required check. Verification uses the ordinary tool boundary and is cancellable. |
| TASK-03 | Independent review receives the actual patch, relevant source, task requirements and verification evidence. Findings have identifiers, severity, evidence and resolution state. The runtime routes findings to the responsible agent within a finite correction allowance set by task policy. Missing evidence is an explicit incomplete review. | Present a patch with a known defect and a misleading worker summary. A blocking finding keeps acceptance closed through correction and follow-up review. Exhausting the allowance leaves the task awaiting a developer decision, not silently accepted. |
| TASK-04 | Outcomes distinguish completed execution, verified work, review status and developer acceptance. Integration records the exact accepted patch and refuses stale or conflicting changes. Commit, merge and push obey the developer's chosen policy. | Accept a verified patch, then change the target before integration. Require reconciliation instead of overwriting it. Neither a model conclusion nor an unchecked merge may set the task to accepted. |
| TASK-05 | Resource limits and usage are cumulative for the task across retries, resume, children, advisor, review and compaction work. Admission observes the remaining limits. Unknown usage/cost stays unknown; unsupported hard limits are rejected as unsupported. | Exhaust a small fixture allocation through auxiliary calls, restart, and request more work. No reset or hidden allowance is permitted. A hard monetary ceiling must not be promised when price or billable usage cannot be bounded. |

Proposed default: checks required by an agreed task plan block verified
completion. Independent review is available in M2 and required when the selected
task policy says so; ordinary small changes do not need a panel of models.
Developer acceptance remains explicit unless the developer has already selected
an automatic acceptance/integration policy with a defined scope.

Budgets are optional configured task controls, separate from provider model
output/context capabilities. A task limit must not silently become a small fixed
per-response output cap. Proposed time-limit semantics: cumulative task running
time across attempts, with overlapping child work counted once as elapsed time.
An acknowledged pause stops that clock only after owned work has stopped.
Uncertain running intervals after a crash are charged conservatively until
reconciled, with the uncertainty visible. Requests and tokens sum all participating
roles. Resuming the same task does not reset any of these counters.

## 8. Extensions that serve project work

| ID | Required behavior | Acceptance case and falsifier |
|---|---|---|
| EXT-01 | Extensions have a manifest, identity, version, source and declared contributions/capabilities. The developer can inspect, install, enable, disable and remove them at user or project scope. | Install a local fixture package and an explicitly selected pinned remote package. A version mismatch is visible; merely opening a repository must not enable executable project content. |
| EXT-02 | Extensions can contribute commands, skills, prompt templates and tools through versioned contracts. Discoverability and availability reflect the active project and connection. | A fixture extension contributes one command, one skill and one harmless tool used in a real session. Disabling it removes those contributions without corrupting session history. |
| EXT-03 | Lifecycle hooks have ordered, bounded effects and declared ownership. Final transformed actions still pass runtime admission. An extension cannot rewrite original results or launch untracked model work. | Transform a permitted request into a denied one, time out an admission hook and alter a result's presentation. No denied effect occurs, the timeout is visible, and the original failed result remains unchanged. |
| EXT-04 | An optional MCP bridge exposes explicitly enabled server tools with origin labels, capability limits, timeouts and cancellation. Server tools receive no implied permission beyond their declared execution boundary. | Enable a local fixture server, call and cancel a harmless operation, then disconnect it. The terminal stays usable and unavailable tools are withdrawn. A claimed confined tool whose server bypasses confinement fails. |

The first extension milestone does not need a marketplace, arbitrary in-process
dynamic libraries, or compatibility with every pi/OMP extension. Executable
extensions must have an explicit trust and isolation model; an in-process trusted
module must never be advertised as sandboxed. Browser, debugger and web-research
tools can use this surface when a concrete project workflow selects them.

## 9. Models, advisor and visible agents

| ID | Required behavior | Acceptance case and falsifier |
|---|---|---|
| AGENT-01 | Main agent, advisor, reviewer, compactor and workers can have independent connection/model/effort assignments. Changes take effect at a declared safe boundary and preserve attribution. | Assign distinct fixture connections, change one while idle and inspect actual requests and usage. An unsupported model change or silent billing-method switch fails. |
| AGENT-02 | An advisor observes selected task checkpoints and offers cited guidance without owning execution. It can be invoked directly and, when enabled, after plan changes or failed verification; duplicate evidence does not trigger duplicate advice. | Give the advisor a failing check and inspect its recommendation in the parent's next decision context. Advice cannot execute tools, become a developer instruction or disappear without acknowledgment. Its work consumes the task allocation. |
| AGENT-03 | Each child has an objective, owner, context, permitted tools, file boundary, allocation and completion criteria. The developer can inspect, steer and cancel it while the parent continues independent work. | Delegate a read-only investigation and a bounded edit, observe both, and cancel one. A hidden child, overlapping undeclared mutation or unusable parent editor fails. |
| AGENT-04 | Mutating children use isolated workspaces. Integration is explicit, reviewed and checked against the parent's current state. Read-only children cannot mutate through another tool route. | Give two children overlapping changes and modify the parent independently. Produce an explicit conflict; never choose a winner by overwriting files. |
| AGENT-05 | Agent messages preserve sender, recipient, assignment and delivery identity. Queued, delivered and applied are distinct where applicable. Waiting does not confuse unrelated traffic with an expected reply. | Interleave peer replies, a developer correction, cancellation and restart. No message acquires another sender's authority or resolves an unrelated wait. |
| AGENT-06 | Concurrency, descendant depth and cumulative resources are bounded. Parent cancellation reaches descendants; recovery lists interrupted child work and its effects. | Reach a small configured concurrency limit, cancel the parent and restart. No orphan work continues past the grace period and no child gains a fresh parent allocation. |

The existing **Oracle** screens certain tool requests under explicit host access.
The **advisor** recommends how to do the task. The **reviewer** evaluates results.
These responsibilities remain distinct even if the developer assigns the same
model to more than one role.

## 10. Coordination and evidence-based improvement

| ID | Required behavior | Acceptance case and falsifier |
|---|---|---|
| ORCH-01 | A runtime-owned dependency graph admits ready assignments, identifies blocked work and rejects cycles. Agents propose plan changes; they do not directly mark dependencies verified or integrated. | Run independent nodes concurrently and hold a dependent node behind a failed check. Early admission or acceptance based on a worker's self-report fails. |
| ORCH-02 | Stalls and repeated failures produce cited observations and bounded recovery choices. Redirects, retries and model changes preserve the original task and allocation. | Repeat the same failure, exhaust a configured recovery allowance and restart. The runtime must stop or request a decision instead of cycling indefinitely. |
| ORCH-03 | Contested blocking findings can use a critic/defender/judge process under an explicit task policy. Each role receives the actual evidence; the final resolution and dissent remain recorded. | Use a planted defect and a disputed false positive. No participant can erase the failing check, grant permission or resolve missing evidence by a confidence score. A bounded unresolved dispute returns to the developer. |
| LEARN-01 | Runtime observations and developer annotations can become cited deficiencies and proposed correction tasks. A candidate contains its evidence, expected change, falsifier and risks. | Generate recurring fixture friction and inspect the candidate through the terminal. A candidate without traceable evidence, or one that starts coding by discovery alone, fails. |
| LEARN-02 | Relevant lessons are retrieved with their sources and scope, and the developer can inspect, disable or supersede them. Lessons are evidence, not elevated instructions. | Produce a lesson through a completed correction, start a related task and inspect the actual context. An unrelated or superseded lesson must not silently govern the new task. |
| LEARN-03 | Authorized harness improvements use the ordinary task, verification, review and integration path, then record whether later evidence supports the intended benefit. | Run a bounded self-improvement case through the production workflow, including a negative check. Bypassing controls because the target is DemonCoder, or claiming improvement from an unexecuted plan, fails. |

Default proposal: advisor feedback is opt-in; ordinary review uses one independent
reviewer; the critic/defender/judge panel is reserved for explicitly selected
contested findings. No role receives extra execution authority because it is
called an advisor, judge or supervisor.

## 11. Ownership and compatibility

| Owner | Responsibility |
|---|---|
| Terminal | Render state and submit typed commands; never wait for work to finish inside input handling. |
| Session/task runtime | Admit commands, own lifecycle and allocation, persist authoritative state, recover and publish outcomes. |
| Native loop or selected external backend | Own exactly one model/tool progression loop per agent session. |
| Tool/extension boundary | Validate final requests, enforce access, preserve results and cancel owned operations. |
| Verification/review coordinator | Bind checks and findings to actual workspace evidence and evaluate the selected acceptance policy. |
| Agent coordinator | Admit children, route messages, enforce ownership and integrate accepted results. |
| Evidence/learning component | Derive inspectable observations and lessons without granting execution authority. |

These are responsibilities, not a prescribed crate count or a requirement for a
new daemon or database. Prefer one authoritative durable representation with
derived views. Storage format and extension transport are implementation choices
to justify against recovery and compatibility requirements.

New features publish a per-connection capability matrix. Existing four-connection
coding behavior remains supported. A connection that cannot perform a new feature
must report that limitation before starting it; this does not count as delivering
that feature for a milestone that requires parity. Each selected commitment names
its required connections and the tested behavior on each.

## 12. End-to-end release scenarios

1. **Everyday repair:** open a dirty temporary project, find a defect, edit it with
   current anchors, observe a failing test, correct it, inspect the task-only diff
   and required review, then accept. Preserve unrelated work.
2. **Long session:** compose and queue work while a tool streams, inspect context,
   compact it, close the application, resume and continue with the same pending
   constraints, usage and workspace identity.
3. **Interrupted mutation:** stop the process after a harmless tool effect but
   before its receipt. On resume, reconcile the result without applying it twice.
4. **Project extension:** enable a fixture extension, discover its command and
   tool, exercise admission and cancellation, disable it and resume the session.
5. **Delegated change:** assign two independently configured children, inspect
   messages and advisor feedback, cancel one, and integrate the other only after
   checks and review; demonstrate a conflicting integration as a separate case.
6. **Harness improvement:** a real recorded failure generates a cited candidate;
   an authorized correction follows the same coding and acceptance path, and a
   later task receives the applicable lesson.

Use temporary repositories and harmless fixtures by default. A real developer
repository must be explicitly selected for a harness exercise. Retain failed
cases and limitations as well as passes. No release claim rests on component
tests alone, and no security case requires a destructive host probe.

## 13. Decisions for developer review

| Choice | Proposed default | What choosing differently changes |
|---|---|---|
| Delivery priority | M1 then M2: reliable solo work; retain M3–M5 as explicit follow-on milestones. | Delegation first requires moving its persistence and acceptance prerequisites forward too. |
| Workspace policy | Main agent works in the selected project while preserving pre-existing changes; mutating children use isolated workspaces. | Isolating the main agent too adds a task integration step and changes how existing dirty work is carried forward. |
| Acceptance policy | Required checks block verified completion; independent review is selected by task policy; developer accepts unless a scoped automatic policy already exists. | Mandatory review for every turn adds time and model usage; automatic acceptance needs an explicit boundary. |
| Advisor and panel | Opt-in checkpoint advisor; one ordinary reviewer; panel only for selected disputes. | An always-on advisor or mandatory panel adds calls and latency to ordinary work. |
| Extensions | Explicitly enabled, versioned packages with declared trust/capabilities; no automatic execution from project discovery. | Automatic project activation trades convenience for executing repository-provided code on open. |

Reviewing this draft can approve the feature direction and change these defaults.
It does not authorize implementing all milestones at once. The next step is to
select the first delivery slice, move its agreed requirements and falsifiers into
the normative specification, and let Cairn govern that commitment.

If this isn't clear, ask me to explain it another way before you decide.
