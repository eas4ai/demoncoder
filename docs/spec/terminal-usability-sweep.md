# Terminal usability and reliability sweep

Status: Agreed 2026-09-07
Prefix: SWEEP

The developer selected the remaining recovered sweep after Creator identity and
chat presentation completed. `docs/proposals/normal-task-usability.md` preserves
the already confirmed intent and the wrong-repository provenance. This contract
uses Demoncoder's existing session, selected workspace and access policy.

[SWEEP-001] The investigation MUST retain a separate evidence-backed disposition for the reported PTY/temporary-storage, namespace, fixture-presentation and skill-read failures.
Falsifier: A report is omitted, a symptom is called fixed without reproduction and a correction check, an unavailable original command is presented as known, or confinement is weakened to obtain a pass.
Mechanism: sweep-investigation; production-executor cases in disposable directories, the existing presentation-failure regression, and a cited investigation record including exact limits.

[SWEEP-002] The terminal MUST animate a working indicator during silent active waits.
The terminal MUST retain useful active tool intent beside the indicator when available.
The indicator MUST stop when the turn is terminal.
Falsifier: Successive screens during a held provider/tool wait show a static indicator, the active tool target disappears, or animation continues after completion or cancellation.
Mechanism: sweep-interaction; deterministic view tests and held-provider PTY screens. Existing Oracle waits are covered through active tool state; no new planning/review phase is implied.

[SWEEP-003] Dragging the transcript scrollbar MUST move the current compact or full viewport.
New output MUST preserve the developer's manually scrolled anchor.
Falsifier: Press/drag/release on the visible rail does not move the viewport, endpoints or resized geometry are wrong, or incoming output steals a position chosen above the live tail.
Mechanism: sweep-interaction; production mouse events, compact/full geometry, resize, incoming output and terminal screens.

[SWEEP-004] The terminal MUST support mouse selection of visible transcript text for explicit copying.
Incoming output MUST NOT change the selected text.
Selection storage MUST remain bounded.
Falsifier: A drag cannot select Unicode text, output or a final tool receipt changes the selected bytes, copy occurs without an explicit gesture, or selection retains unbounded history.
Mechanism: sweep-interaction; press/drag/release through the real terminal, frozen visible rows, Unicode/reverse selection, incoming replacement, resize, explicit clipboard escape and byte bounds.

Selection freezes the visible transcript snapshot until an explicit action clears
it. Ctrl-Y copies selected displayed text through OSC 52; terminal clipboard
support is reported as a capability limit, not guaranteed by the application.
Escape clears selection without cancelling a turn. Scrolling, expansion, a new
selection or End clears the snapshot. The underlying session continues normally.

[SWEEP-005] The terminal MUST show a compact strip ordered as model, context used and remaining capacity, dirty-file count and active branch, diff additions/deletions, active subagents, and existing token counts.
Git information MUST describe the selected workspace.
Status collection MUST keep terminal input responsive with bounded work.
Falsifier: An available field is missing or out of order at adequate width, a different repository supplies Git values, Git blocks the input loop, or a zero child count implies implemented subagent execution.
Mechanism: sweep-status; production view tests, temporary Git repositories including unusual filenames, missing Git/HEAD, bounded command failure and PTY screens. Narrow screens may clip trailing fields while retaining the editor and controls.

[SWEEP-006] Context display MUST distinguish the current request/context from cumulative billed input.
The display MUST label estimates and leave unavailable capacity unknown.
A supplied positive context-window override MUST determine displayed capacity.
Falsifier: Repeated requests add cumulative usage into occupancy, cached Anthropic input is omitted from a claimed exact count, an estimate is labelled measured, an unavailable limit is guessed, or a new turn/model retains a stale measured context value.
Mechanism: sweep-status; actual native request estimates, current-request provider/backend usage fixtures, unknown fields, explicit capacity, overflow and repeated-turn cases.

[SWEEP-007] Normal and Oracle usage displays MUST omit monetary cost when the complete reported cost is unavailable.
The displays MUST retain known zero cost and independently known token counts.
Falsifier: Unknown/partial pricing renders a monetary field, missing values become measured zeros, or known zero is suppressed.
Mechanism: sweep-status; known, partial, absent and zero usage through rendered cells and all four adapter terminal fixtures.

[SWEEP-008] The specification and user documentation MUST agree with this sweep's delivered behavior and pass the installed specification lint.
Falsifier: The recorded eight lint findings remain, controls/status semantics are misdocumented, or ignored/local/live checks are described as passing evidence without execution.
Mechanism: sweep-docs; specification lint and manual review of the controls, context, clipboard, investigation and verification records.

All task exercises use disposable repositories and controlled providers. Original
screenshot commands are unavailable unless recovered by investigation. The
existing host Unix socket, queue backpressure, argument-bound and UTF-8 source
review inventory remains separate backlog work; this sweep does not claim those
reports resolved without their own reproductions. Subagent execution, native
search, durable recovery and the broader harness proposal remain later scope.
