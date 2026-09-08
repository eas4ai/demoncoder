# Provider setup and agent assignments

Status: Agreed 2026-09-08
Prefix: SET
Host paths: ~/.demoncoder/ /settings

The developer confirmed provider checkboxes with authentication checks, model
selection after provider selection, a list of defined agent types inheriting the
Creator model, Enter to assign a selected model, and an in-session Settings screen.
They also confirmed that inherited assignments follow Creator changes and that
running work keeps its model while later work uses the new assignment.

## User flow

1. Select one or more provider/authentication choices. OpenAI API, Anthropic API,
   Codex subscription and Claude subscription remain distinct choices. Show each
   selected choice's checking, authenticated or unavailable state and a useful
   recovery action. API credentials are masked; subscription access checks both
   the installed CLI and its login, rather than treating installation as login.
2. Select the Creator model from models offered by the selected authenticated
   providers. Label the provider and authentication method beside the model so
   an identical model name cannot hide a different account or billing route.
3. Show Creator and the existing Worker, Oracle, Reviewer, Advisor and Judge roles.
   Explain each role briefly; Oracle outside-access decisions and Advisor review
   remain separate responsibilities. Every role other than Creator initially
   says "Use Creator model" and shows the resolved model. Selecting a role opens
   a model list. Enter assigns the selected model and returns to the role list;
   Escape returns without changing the assignment. Continue accepts all remaining
   inherited defaults. Assignment does not itself enable orchestration or work.
4. Open `/settings` during a coding session to revisit the same provider and
   assignment controls. A confirmed assignment is persisted and becomes the
   default for subsequent work. Work already admitted keeps its captured model,
   account, permissions and allocation. Show when a change will take effect.

## Requirements and falsifiers

[SET-001] Onboarding MUST present independently selectable provider/authentication checkboxes before model selection.
Onboarding MUST show the outcome of authentication checks for selected providers.
An API choice MUST validate its selected key through its provider.
A subscription choice MUST check the CLI and its managed login. Failed, missing, cancelled or timed-out authentication MUST NOT appear authenticated. Checks MUST NOT start a coding task or silently change authentication method.
Falsifier: Model selection precedes provider selection, CLI presence alone establishes login, an invalid key is marked valid, or an auth check starts coding work or switches billing routes.
Mechanism: Drive first-run setup through a PTY with controlled API and CLI authentication fixtures; exercise valid, missing, rejected and slow credentials and inspect requests and saved choices.

[SET-002] Creator and role model selectors MUST list models from selected authenticated providers, with unambiguous provider/authentication identity and the current selection.
The runtime MUST obtain model choices from provider/backend discovery or explicitly identified supported backend choices, report discovery failures, and reject an unavailable selection without silently substituting another model. It MUST NOT imply that an authenticated account guarantees access to every advertised model.
Falsifier: An unselected or unauthenticated provider contributes selectable models, model identity loses its provider, unavailable discovery appears complete, or the actual request uses a different selection.
Mechanism: Supply distinct and overlapping model catalogs, unavailable discovery and provider rejection; navigate the real selector and inspect the selected adapter request.

[SET-003] Onboarding and Settings MUST list every defined user-assignable agent role and allow Enter to assign a model or accept the Creator default.
Oracle, Reviewer, Advisor, Worker and Judge responsibilities MUST remain distinct. Cancelling a model selector MUST preserve its previous assignment. Users MUST be able to finish onboarding without individually visiting every role.
Falsifier: A runtime role has no assignment row, choosing a row changes another role, Escape saves a highlighted option, or unchanged defaults require repeated manual assignment.
Mechanism: Use keyboard-driven production terminal cases to assign different models to roles, cancel a selection and finish with defaults; inspect persisted assignments and role requests.

[SET-004] A role using the Creator default MUST inherit subsequent Creator model changes.
An explicit override MUST remain unchanged until the developer changes it or restores inheritance.
The application MUST show inherited versus overridden state and the effective provider/model. Model assignment MUST NOT confer tool authority or enable work, delegation, review or orchestration by itself.
Falsifier: Changing Creator leaves an inherited role on an old copied value, changes an overridden role, or model assignment changes a role's permissions or starts work.
Mechanism: Change Creator between two configured models, exercise an override and restore-default action, and inspect effective requests and unchanged execution controls.

[SET-005] The developer MUST be able to open Settings and change provider selections and agent assignments without restarting the application.
The screen MUST use the same assignment semantics as onboarding, preserve the prompt draft and conversation position, and remain usable during work, resize, cancellation and bounded authentication/model discovery.
Falsifier: Settings requires restart, loses a draft or scroll position, interrupts running work merely by opening, or a slow provider blocks navigation or cancellation.
Mechanism: Open Settings in a production PTY while a controlled task runs; edit an assignment, navigate back, resize and cancel slow discovery while verifying session and draft continuity.

[SET-006] Each admitted task or agent invocation MUST capture its effective connection and model.
Later Settings changes MUST apply to subsequent work without altering work already admitted.
The runtime MUST preserve the original identity of queued, running and interrupted work, original results and consumed allocation. A model or provider change MUST NOT replay work, reset allowance, erase evidence or claim that an opaque backend session transferred between providers. Existing explicit command-line or per-assignment overrides MUST remain visible and take precedence where they apply.
Falsifier: An in-flight operation changes model after a Settings save, subsequent eligible work ignores the new assignment, a queued assignment changes silently, or switching resets authority, allocation or retained evidence.
Mechanism: Hold real fixture requests across a Settings change, then create new work; compare actual old/new provider requests, retained identities, allocations and restart behavior, including external backend limits.

[SET-007] Settings changes MUST persist through the existing private settings authority with atomic publication, credential-safe errors and explicit refusal of invalid or conflicting updates.
Existing connection configurations and explicit CLI selections MUST remain usable. Environment API keys MUST keep precedence over saved keys. Deselection MUST NOT silently replace an existing role's provider.
Unresolved assignments MUST be visible and block affected new work until repaired. A failed save MUST NOT be reported as applied.
Falsifier: A partial or failed save becomes active, concurrent edits are silently lost, migration discards an explicit assignment, a secret reaches events or tools, or provider removal redirects a role without consent.
Mechanism: Exercise existing and new configurations, restart, competing saves, cancellation, invalid settings, provider removal and synthetic credentials through the production save and selection paths.

[SET-008] Documentation and the installed terminal MUST accurately describe provider checks, model availability, role defaults, overrides and when live changes take effect.
The implementation MUST preserve existing authentication, confinement, review, integration, recovery and lesson-context boundaries. The final review MUST examine gaps in the mechanisms.
The installed release MUST demonstrate the complete onboarding-to-live-reassignment workflow.
Falsifier: The installed program lacks the workflow, documented controls differ from actual controls, identity/status reports the wrong model, or assignment changes bypass an existing execution or evidence boundary.
Mechanism: Run production onboarding and Settings cases against the installed binary, refresh inherited checks, inspect actual requests and complete a recorded final review with no open findings.

## Scope and implementation guidance

The confirmed scope is provider selection, model selection, role assignment and
live Settings. It adds no provider marketplace, new agent roles, billing manager,
automatic login flow, new execution authority or general plugin settings system.
Use the existing terminal and private configuration facilities. Preserve explicit
project trust and the separation between subscription login and API credentials.
Authentication status is a checked observation, not a permanent guarantee.

The developer requested the oh-my-pi Settings interaction as a reference. The
initial source search found the original pi selector at
`reference/pi_agent_rust-main/legacy_pi_mono_code/pi-mono/packages/coding-agent/src/modes/interactive/components/settings-selector.ts`:
a current-value settings list opens a bounded selection submenu, Enter selects,
and Escape returns. The exact oh-my-pi checkout location has been requested;
this original-pi source is not being represented as oh-my-pi. No reference source
has been copied. Source attribution and any reuse obligations belong in the
implementation decision if code is copied later.

## Commitment

Name: provider-agent-settings.
Requirements: SET-001 through SET-008.
Inherited checks cover connection/authentication routing, private setup storage,
verification/recovery, subagent integration, orchestration, terminal inspection
and learning context. Done requires current passing evidence, a clean final
review and an installed production workflow. The developer confirmed the design
on 2026-09-08; these falsifiers make that behavior observable without adding new
product scope.
