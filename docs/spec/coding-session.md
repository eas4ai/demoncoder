# Coding session

Status: Agreed 2026-09-06
Prefix: CODE

This domain specifies the first usable session. The initial platform is
Linux, matching the developer's current environment. Platform expansion is
later scope. Native API connections use the small Rust loop; external agent
connections are evaluated against the same observable session requirements.

[CODE-001] The application MUST accept coding prompts through an interactive terminal session.
Falsifier: A developer cannot submit a prompt through the actual terminal application and observe the selected runtime start the turn.
Mechanism: coding-session; a pseudo-terminal driver opens the production executable and submits a prompt through its editor.

[CODE-002] The session runtime MUST provide read, write, edit, and bash operations for coding work.
Falsifier: The selected connection cannot complete a controlled read, file creation, edit, and verification-command cycle in a temporary repository through the production tool path.
Mechanism: coding-session and the per-connection cases described in docs/commitments/first-coding-session.md; compare actual files and command output with the requested changes.

[CODE-003] The terminal interface MUST remain responsive while displaying incremental assistant and tool output.
Falsifier: Assistant or tool output appears only after the turn finishes, or the editor cannot accept input while a controlled provider or tool operation is still waiting.
Mechanism: coding-session; hold a fixture response and tool open after their first output, then observe the rendered output and an editor-input acknowledgement before releasing either operation.

[CODE-004] The session runtime MUST apply a developer correction at the next safe tool boundary.
Falsifier: After a correction is queued and the current tool finishes, the runtime admits another queued tool from the superseded response before making the correction available for the next model decision.
Mechanism: coding-session; queue a correction during a controlled tool, then check the acknowledgement, next model input, and absence of the superseded tool's harmless marker.

[CODE-005] The session runtime MUST stop a cancelled turn within its documented cancellation grace period while keeping its session usable.
Falsifier: An owned model request or tool subprocess continues work after the grace period, or cancellation destroys the session so the developer cannot submit another prompt.
Mechanism: coding-session; cancel during provider streaming and during a tool with a child process, inspect process completion and marker activity, then submit a new prompt in the same session.

The proposed default cancellation grace period is two seconds. Tests use
bounded local fixtures and report timing failures separately from missing
test prerequisites. A provider-side billing outcome after a network abort
may be unknown; that uncertainty is shown rather than reported as zero.

[CODE-006] The session runtime MUST retain the preceding turn's relevant conversation and workspace state for the next prompt in the same session.
Falsifier: A second prompt loses the preceding task's context or starts from files that omit its completed changes.
Mechanism: coding-session; create a uniquely named function in the first turn and extend that function in the second, checking both the provider input and resulting repository.

[CODE-007] The session runtime MUST enforce the authorized tool-access boundary on the final tool request.
Falsifier: A denied tool executes, a hook-modified path or command bypasses admission, or a tool changes an unauthorized sibling fixture, reads a protected credential fixture, or receives provider credentials in its environment.
Mechanism: coding-session; use harmless canaries, a denied marker write, transformed arguments, and a synthetic credential to test admission through the production tool executor.

Hooks can add context or request work. The runtime validates their final
arguments and applies permission checks before execution. External backends
need an equivalent tested boundary for their actual tool path. An adapter
that cannot provide it has not met this requirement.

[CODE-008] The session runtime MUST retain the actual result of each completed tool operation.
Falsifier: A failed verification command is recorded as successful, a presentation hook replaces its original evidence, or a tool result is attributed to the wrong call.
Mechanism: coding-session; execute a meaningful failing repository check followed by a correction and passing check, then compare the original results, identifiers, and visible outcome.


[CODE-009] First interactive startup MUST guide the developer through connection/authentication, model, effort, and project trust settings and save the choices privately. The developer MUST be able to reopen setup. An explicit trusted configuration may supply these choices for an automated invocation.
Falsifier: A first-time developer must author a configuration file before submitting a prompt, a saved choice is ignored on the next start, an untrusted project starts tools without authorization, or credential input/persistence exposes a key.
Mechanism: coding-session; drive first startup and repeated startup through a pseudo-terminal, inspect private saved settings, exercise cancelled setup and denied project trust, and submit a coding prompt with the saved connection.

[CODE-010] An explicit --yolo invocation MUST run tools with host access without sandboxing or routine permission prompts, while retaining a blocking Oracle guard for access outside the selected project and session-owned temporary scratch directory.
Falsifier: Host access is silently sandboxed, an outside file operation or unrestricted shell command executes without its required final-request review, a failed/uncertain Oracle allows execution, the Oracle itself can execute tools, or ordinary session scratch work is denied merely for being under /tmp.
Mechanism: coding-session; use temporary projects, scratch directories, outside canaries, controlled Oracle responses and at least one retained live Oracle verdict pair. Verify allowed effects, denied effects, fail-closed errors and cancellation, final hook arguments, explicit access-mode display, and actual host execution. Destructive home/system examples are verdict-only and are never executed.

The default confines file changes to a trusted project and session-owned scratch
and tool-cache storage. Ordinary source/documentation reads and networking are
allowed under the later developer-usability correction. --yolo is an explicit
invocation choice; repository text cannot enable unrestricted host writes. Host execution still
retains typed tools, bounded outputs, cancellation, and original results.
All unrestricted shell requests receive Oracle screening because their
outside effects cannot reliably be inferred from their working directory.
The Oracle sees the developer task, final request, and authorized paths as
separate evidence. It has no tools. Its judgment reduces mistakes and is not
a filesystem isolation boundary. Denial or unavailable review stops the
request and displays a reason.

Session-owned scratch space under /tmp is approved for normal temporary
work and is exposed through TMPDIR. An outside source does not become
approved merely because its destination is scratch space. Setup must not
automatically enable unrecognized plugins or hooks. External extension
loading remains pending; the current build exposes no dynamic loader.

These checks do not establish arbitrary coding-task correctness. Crash
recovery, complete review policy, cumulative task budgets, and advanced
orchestration have separate later commitments. The first session still
reports failures honestly and enforces its declared tool-access boundary.
