# Evidence-based improvement

Status: Agreed 2026-09-07
Prefix: LEARN

The developer selected the next roadmap commitment after status and decision
remediation was completed and installed. The developer confirmed these detailed
requirements and falsifiers on 2026-09-07.
Sources: docs/spec.md, Self-improvement; docs/proposals/developer-harness.md,
Coordination and evidence-based improvement; docs/spec/roadmap.md.

## Intended workflow

A failed verification check becomes a cited observation. The developer can add
an explanation of what went wrong or annotate other retained task/agent evidence.
The developer inspects a bounded correction proposal, chooses its checks and
authorizes an ordinary task. Verification and review establish the actual outcome.
The developer can then approve a scoped lesson for later work. A later matching
task receives that lesson with its sources; unrelated work does not.

An observation records what happened. A candidate proposes a correction. An
outcome records what the correction demonstrated. A lesson is a reusable statement
supported by that history, not another source of execution authority.

## Requirements and falsifiers

[LEARN-001] The runtime MUST derive failed-check observations from original retained verification receipts.
The runtime MUST retain developer annotations with their author and cited task or agent evidence.
The runtime MUST preserve each observation's workspace, source identity, snapshot or generation, and original result.
The detector MUST avoid creating duplicate observations for the same source receipt on refresh or restart.
Falsifier: A fabricated result is treated as an observed failure, a citation cannot resolve to its original result, an annotation changes that result, or the same receipt produces repeated observations.
Mechanism: Produce actual failed checks and annotations through the terminal; inspect their citations, repeat detection, restart, and supply invalid or missing references.

[LEARN-002] The terminal MUST expose improvement candidates with supporting observations, a bounded correction objective, proposed scope, expected benefit, behavioral check and risks.
The runtime MUST distinguish a proposed explanation from an observed fact.
Candidate discovery and inspection MUST NOT execute coding tools or start correction work.
The runtime MUST admit any requested model-assisted proposal through explicit invocation and the existing allocation and cancellation controls.
Falsifier: A candidate lacks traceable evidence or a behavioral check, invented causes appear proven, browsing starts work, or proposal generation makes hidden or unaccounted model calls.
Mechanism: Inspect a production-generated candidate; reject absent evidence; count provider requests, admissions and filesystem effects before and after browsing and explicit proposal generation.

[LEARN-003] The runtime MUST require an explicit developer command to turn a selected candidate into a correction task.
The runtime MUST attach the candidate and its cited evidence to the actual task context.
The correction task MUST use the existing permissions, selected checks, review, acceptance and allocation rules.
Child integration MUST retain its existing explicit developer gate.
Falsifier: Candidate or agent text authorizes execution, the actual worker request omits the cited evidence, or improvement work bypasses an ordinary task or integration gate.
Mechanism: Create a correction through the production terminal, inspect the actual worker request, then exercise absent authorization, failed checks, stale files and a ready child awaiting integration.

[LEARN-004] The runtime MUST record the correction outcome against the original candidate and executed behavioral check.
The runtime MUST distinguish supported improvement, unresolved deficiency and insufficient evidence.
Task acceptance MUST NOT by itself establish the candidate's claimed benefit.
The runtime MUST retain failed and abandoned correction outcomes beside successful ones.
Falsifier: A plan or unrelated passing test becomes proof of improvement, the original behavioral failure remains while the candidate is marked addressed, or later success erases an earlier failure.
Mechanism: Run a planted failing behavior, an accepted but ineffective correction, and an effective correction through the existing workflow; compare original and subsequent check evidence.

[LEARN-005] The runtime MUST require explicit developer approval before enabling a lesson for later work.
The runtime MUST retain each lesson's claim, workspace scope, applicability, source observations and correction outcome.
The developer MUST be able to inspect, disable and supersede a lesson without deleting its source history.
The runtime MUST reject an enabled lesson that claims a verified correction without supporting outcome evidence.
Falsifier: An unapproved or unsupported lesson becomes active, scope or sources are missing, or disabling or superseding a lesson erases the original evidence.
Mechanism: Create and approve a lesson from a production correction; attempt unsupported activation; disable and supersede it; reopen its original evidence after restart.

[LEARN-006] Context preparation MUST retrieve enabled applicable lessons when creating later coding work in the selected workspace.
Context preparation MUST include each supplied lesson's identity, scope, source and selection reason in inspectable task evidence.
Context preparation MUST exclude unrelated, disabled, superseded and other-workspace lessons.
Context preparation MUST label lessons as evidence subordinate to developer instructions and applicable repository instructions.
The runtime MUST document its repository-instruction precedence and scope at task creation.
Falsifier: A matching approved lesson never reaches the actual provider request, irrelevant or disabled material is injected, a lesson becomes higher-priority authority, or repository instruction scope is lost.
Mechanism: Generate a lesson through the full correction workflow, start matching and unrelated work on all four connections, and inspect actual adapter requests rather than calling a prompt helper directly. Include conflicting repository instructions and a fresh session in the same workspace.

[LEARN-007] Durable recovery MUST preserve observation, candidate, correction and lesson identities and their relationships.
Restart MUST NOT create a second correction from the same recorded authorization or replay interrupted work automatically.
The runtime MUST retain consumed allocation and existing connection authority when resuming an improvement correction.
The runtime MUST show an actionable refusal when an evidence source is missing, damaged or outside the selected workspace's authority.
Falsifier: Restart duplicates work, resets an allowance, silently drops provenance, treats inaccessible evidence as valid, or repeats an uncertain effect.
Mechanism: Kill the production process at candidate authorization and correction boundaries; resume and inspect requests, identities, evidence and effects. Exercise missing sources, old records and finite storage limits.

[LEARN-008] Improvement inspection and context preparation MUST have explicit finite limits on retained content, selected lessons and per-request work.
The terminal MUST preserve usable input and cancellation during discovery, inspection and correction.
The terminal MUST distinguish saved evidence from a fresh verification of files.
The runtime MUST report incomplete retrieval or storage refusal without manufacturing a complete result.
Falsifier: Large history blocks cancellation, retrieval grows without a declared bound, evidence disappears silently, or old results appear freshly verified.
Mechanism: Use large Unicode evidence, many observations and lessons, slow or unavailable state, narrow terminal layouts and interrupted work through the production application.

## Scope and choices

- Start with a deterministic failed-verification detector and explicit developer
  annotations. Broader stall, cost and recurring-friction detectors remain later
  extensions; they are not prerequisites for demonstrating the complete workflow.
- Use existing terminal commands and the bounded inspector for candidates,
  outcomes and lessons. No new application, daemon or autonomous repair loop.
- Correction tasks retain the current native OpenAI/Anthropic parent workflow.
  Child work and lesson delivery cover all four existing coding connections.
  This commitment does not claim new opaque-backend recovery or allocation powers.
- Scope reusable lessons to the selected workspace, across its sessions. Matching
  uses explicit applicability metadata and a documented deterministic rule; no
  universal quality score, cross-project sharing or embedding service is required.
- Repository instructions are scoped input, not permission grants. Document and
  test selected-workspace instruction loading and precedence alongside lesson
  delivery. Arbitrary instruction imports and executable project plugins are outside
  this slice. Exact file-selection rules will be recorded before implementation.
- Keep original source receipts authoritative. A missing source is visible and
  blocks claims dependent on it; a copied summary cannot replace missing proof.

## Commitment

Name: evidence-based-improvement.
Requirements: LEARN-001 through LEARN-008.
Inherited regression coverage: VERIFY-001 through VERIFY-006, SUB-005, SUB-007,
ORCH-006, ORCH-007, REM-002 and REM-004.

Mechanism: evidence-based-improvement, with per-requirement results from
a production PTY driver, controlled HTTP/backend fixtures, focused Rust checks
and installed specification lint. Its declaration will include every runtime,
adapter, storage, context, terminal, fixture and documentation input it depends on.
Existing declarations continue to own the inherited checks.

Done when a real failing fixture leads to an inspectable cited candidate, an
explicitly authorized correction, executed evidence of its outcome, an approved
lesson and a later matching task that actually receives the lesson. Negative
cases must demonstrate rejected unsupported claims, no unauthorized work, bounded
retrieval and safe interruption. Every requirement needs current passing evidence,
no open final-review finding and verification through the installed executable.

