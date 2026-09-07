# Recovered terminal usability work

Status: Recovered developer-confirmed intent; Demoncoder implementation mapping for review.
Recorded: 2026-09-07
Source: suprnova-coder commit `6f433cb3a2ad599d60255a9347a58a70ad7a2ac2`,
`suprnova-coder@6f433cb:docs/spec/normal-task-usability.md`,
`suprnova-coder@6f433cb:docs/commitments/normal-task-reliability-and-terminal-usability.md`,
and their promoted backlog. The developer identified the repository mistake in
this session. `handoff.md` records the intended Demoncoder project boundary.

Creator identity is complete, as the developer explicitly corrected in this
session; its pending implementation is now recorded in commit `99e9ae8`.
See `.cairn/reviews/creator-identity.md` for the checks run during reconciliation.
It is not a candidate next commitment.

## Requested behavior and Demoncoder mapping

The original NTU identifiers below are provenance, not newly Agreed Demoncoder
requirements. The current roadmap still names `chat-presentation`; the recovered
work is tracked in `.cairn/backlog/recover-the-confirmed-terminal-usability-sweep-in-demoncoder.md`.

| Original request | Confirmed intent | Current code and proposed falsifier/check |
|---|---|---|
| NTU-006 | Animate the working indicator during silent active waits and retain useful tool intent. Cover planning/review where those phases exist. | `src/terminal.rs:105` stores a static Working label; `src/terminal.rs:297` already ticks every 33 ms. Hold a local provider without text and inspect successive terminal screens; the indicator must advance while work remains active and stop when work ends. Do not invent a planner phase in this session runtime (`src/session.rs:132`). |
| NTU-007 | Drag the transcript scrollbar; new output must preserve a manually scrolled position. | `src/terminal.rs:356` handles wheel events only; `src/terminal.rs:449` draws the rail. Extend `tests/scrollback.py:133` with press/drag/release events, rail endpoints, resize and incoming output. An unchanged viewport or stolen anchor is failure. |
| NTU-008 | Select transcript text with the mouse for copying; incoming output must not change the selection. | `src/terminal.rs:356` has no selection handling; `README.md` describes emulator Shift-selection where supported. Define the actual supported gesture, selection lifetime, Unicode/wrapping and copy behavior, then exercise them through terminal input. A changed selected string merely from streaming is failure. |
| NTU-009 | Order the compact strip as model; current context used and remaining capacity; dirty-file count and active branch; diff additions/deletions; active subagents; existing token counts. | `src/main.rs:23` passes connection/mode text to the terminal; `src/terminal.rs:371` draws the existing header/footer. Feed real selected model/workspace data through that boundary; use a disposable Git fixture with known changes and narrow terminal screens. Missing available fields, wrong order or another repository's values fail. |
| NTU-010 | Current occupied context differs from cumulative billed usage. Show capacity and remaining space only when known; label estimates. | `src/events.rs:14` has usage events but no context occupancy/capacity record. Specify the producing adapter's actual observable values before adding fields. Checks must distinguish repeated billed input from occupied context, unavailable capacity and model changes. Demoncoder has no delivered compaction feature; do not import a compaction lifecycle from the other app (`docs/proposals/developer-harness.md:130`). |
| NTU-011 | Omit monetary cost when unknown or only partly priced; retain a known zero. | `src/terminal.rs:205` prints cost unknown and `tests/usage.py:119` expects it. Change the presentation and assertions together when committed to this requirement. Test absent, partial and known-zero prices; retain the separately known token values. Oracle usage has separate rendering at `src/terminal.rs:152`; the scope should state whether the same display rule applies there. |
| NTU-005 and temporary-storage report | Investigate PTY/read-only temporary storage, disabled namespaces, fixture-presentation failure and blocked skill read separately. Retain exact evidence limitations and preserve confinement. | `src/developer_access.rs:298` builds Demoncoder's sandbox and sets writable scratch as TMPDIR at line 407. `tests/developer_access.rs`, `tests/host_guard.rs` and the terminal drivers are local check seams. Recover originating commands or reproduce safely; a passing unrelated test does not close a screenshot symptom. |

## Repository-specific exclusions and retained constraints

NTU-001 and NTU-002 came from a Suprnova conductor/client/daemon exercise, and
NTU-003's correction changed its owned-worktree capabilities. Demoncoder starts
one session worker in the terminal process (`src/main.rs:14`) and uses its selected
workspace with the default access builder (`src/developer_access.rs:298`). The
other repository's failing runs, implementation commits and passing receipts do
not establish any Demoncoder failure or fix.

Use ordinary bounded tasks in disposable repositories. The developer rejected
live task execution against real user repositories and did not request endurance
testing (`handoff.md`, Pending developer requests; original NTU spec lines 72–74).
Subagent execution and provider-selected native search remain subsequent work;
a count in the status strip must not claim spawning exists (`handoff.md`, Pending
developer requests; original NTU spec lines 65–70).

Before this becomes Demoncoder's next commitment, map the chosen requirements and
falsifiers into its specification and declare checks against its production
paths. The existing developer requests need not be rediscovered or replaced by
the broader developer-harness proposal. Source-dependent choices above remain
explicit, rather than importing the other application's architecture.
