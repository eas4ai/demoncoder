# Terminal usability sweep review

commitment: terminal-usability-sweep
commit: 7b8a8dd9c942ce6aa021607ea8350f75a89320de
findings:
  - none: no open findings within this commitment
Reviewer: Codex
Date: 2026-09-07
Status: complete

## Requirement and mechanism review

Checked the recovered intent against this application's session and event model.
The four screenshot test symptoms are separate from the existing source-review
backlog. The investigation may record missing original evidence, but may not call
such a symptom fixed. No daemon/worktree result from the other repository counts.
The interaction checks observe screens and mouse/clipboard behavior; a static
glyph or unchanged scroll position is a safe violating example. Usage cases must
reject unknown prices, cumulative context and wrong-workspace Git values.
Selection freezes bounded visible rows and keeps its copied bytes stable while
new output is ingested. Narrow/Unicode screens and absent metadata remain part
of the checks. Old lint fixes only split obligations and declare existing host
paths, preserving their substantive meaning.

At mechanism design, implementation and verification were pending. The following
sections retain failing demonstrations and passing corrections.

## SWEEP-001 investigation

The three production-executor probes and existing native presentation regression
pass. `docs/investigations/terminal-sweep.md` records each exact command, observed
result and evidence limitation. Nested user namespaces are refused at uid_map;
no access change was made. The assertion in the public skill case also verifies
that the selected credential canary remains denied. These are observation checks
of existing behavior, so no implementation repair or invented before/after is
claimed for an unavailable original screenshot command.

## Interaction failure demonstrations

Before implementation, `python3 tests/terminal_sweep.py` failed all three cases:
unchanged header during a silent wait, rail drag left the viewport at the live
tail, and mouse selection exposed no copy gesture. After the implementation,
all three pass, as do 25 library tests (including Unicode selection and bounds).
The existing `CopyToClipboard` command from Crossterm's osc52 feature supplies
encoding; no custom clipboard protocol encoder was introduced. The first build
caught the unavailable Line::into_owned method; snapshot ownership now explicitly
copies span text and preserves style/alignment. A stale-binary driver run during
that build failure is not correction evidence.

The new status PTY test fails at the missing selected-workspace Git state.
An explicit negative cost assertion fails on all four adapter usage cases before
the formatter change; a positive substring assertion alone was insufficient and
was strengthened before implementation.

## Additional implementation checks

Current-request context and cost omission passed all four real adapter parsers in
local PTY fixtures. The status tests passed selected-workspace ordering, explicit
capacity and repeated turns; a hung Git configuration FIFO produced the bounded
unavailable status while input and quit remained usable. The Unicode PTY case
initially waited for a header word clipped by its deliberately narrow screen;
it now observes the editor returning to Prompt. Copy bytes remained correct.
The final-receipt, resize, reselection, compact/full rail endpoint and Oracle zero
cases passed in rendered-cell tests. The original eight spec lint findings were
reproduced, corrected without changing their obligations, and lint passed.
DISPLAY-001 now describes the token/cost segment within the selected SWEEP-005
status row; it still forbids unknown-only usage and resets per turn.

Ripwire edit-check reports status_line as a new symbol with no incompatible caller
found. The source-only quality scan exits 2 with 11 major heuristic findings;
test-gate exits 4 and recognizes zero tests, although Cargo executes the embedded
Rust tests and Python drives production adapters. The scan also labels used Rust
methods/types/tests dead and reports a change in untouched adapters/process.rs.
Those are discovery limitations, not passing gates. Actual growth is in terminal
interaction/rendering and adapter event handling. Shared MessageContext extracted
the duplicated Anthropic/Claude usage merge, reducing the added nested branching.
The remaining event variants and mouse/render branches implement selected scope;
no unrelated adapter refactor or synthetic metric baseline was added. The bounded
Git reader resembles the existing native text reader, but reads async pipe bytes
under a separate budget; sharing the filesystem-specific implementation would
couple unrelated boundaries. The public terminal::run wrapper stays for external
adapters and is exercised by the independent registry driver in the mechanism.

Final committed checks, installation and no-code review are recorded below.

## Committed interaction correction

The first committed interaction mechanism retained unverified receipts because
the 60-column scrollback regression could not see completion: the old connection
label consumed the header before activity status. The transcript anchor and input
were correct. SWEEP-002 correction puts activity first and strengthens the silent
wait fixture to 60 columns. This makes the working indicator useful on ordinary
narrow screens rather than weakening the completion observation. The failed
receipt and its exact screen remain in SWEEP-002 evidence.

## Final no-code review

Reviewed the committed diff, source boundaries, current SWEEP specification,
mechanism declarations, receipts, README and investigation record. No production
code was changed during this review. Implementation is in 6ee2e18, with the
narrow-header correction in 51290aa. The reviewed tree is 7b8a8dd.

Attacked the gaps beyond happy-path screens: a final tool receipt replacing
selected output; Unicode display columns and reverse selection; a second selection
on a frozen view; compact/full rail endpoints and resized geometry; unsupported
clipboard capability; configured Git filters; a blocked Git config read; missing
HEAD; cached Anthropic tokens; Claude aggregate result usage; Codex cumulative
usage; unknown, partial and zero money; per-turn context reset; tiny terminals;
and interruption/continuation through the existing session owners.

The selection stores bounded owned visible rows, independently of live chat.
Only an explicit Ctrl-Y emits OSC 52, using Crossterm's encoder. The UI describes
the clipboard operation as a request because terminal policy determines success.
Git runs outside the input loop, has bounded output and a deadline, and kills its
owned command when cancelled. Optional locks, configured filters, fsmonitor,
external diff and textconv are disabled; submodule working contents are explicitly
excluded. No runtime confinement or tool-admission rule changed in this sweep.

Context arithmetic checks overflow and keeps missing capacity unknown. The shared
message accumulator retains only four numeric usage fields. It resets at each
message start and replaces streamed counts, including both kinds of cached input.
Native byte estimates are labelled; backend aggregate billing never becomes
occupancy. Usage and Oracle reports share cost formatting that preserves known
zero and omits unavailable money. The existing terminal::run API remains as a
wrapper; independent adapter registration still works.

The executed committed mechanisms passed all eight requirements. Cargo ran 69
passing tests with three intentionally ignored entry points; the independent
registry driver was then exercised through its PTY driver. Formatting and Clippy
with warnings denied passed. The mechanisms also passed four interaction PTY
cases, three status cases, all four usage adapter fixtures, three chat cases,
two scrollback cases, all four continuation/ownership fixtures, and startup,
onboarding and configuration regressions. Specification lint is clean. Receipts
and complete command output are retained under SWEEP-001 through SWEEP-008,
including the earlier failed interaction observation.

`cargo install --path . --locked` built the release and replaced
`/home/shawn/.cargo/bin/demoncoder`. PATH resolves to that binary; its SHA256 matches
`target/release/demoncoder`, and its help exposes --context-window. The same seven
status/interaction PTY cases passed against the installed release executable.
`.cairn/evidence/terminal-sweep-install.log` retains the path, digest and results.

Limitations remain explicit: original screenshot commands were unavailable;
nested user namespaces are still refused by this environment; terminal OSC 52
support varies; native context estimates are not tokenizer measurements; unsupported
capacity is unknown; Git snapshots can lag; no paid live-provider refresh was run.
Historical live records are not presented as fresh evidence for this tree. The
separate source-review and broader harness backlogs were not silently implemented.
Ripwire's nonpassing heuristic reports and their assessment are recorded above,
separately from executed Cargo/PTY evidence.

The final self-audit covered all fourteen production coding rules: authorized
scope, coherent changes, maintainability, boundary contracts, errors and secrets,
security, lifecycle cleanup, bounded work, tracked completion, actual verification,
honest reporting, partnership, release review and plain documentation. Within
this commitment, the implementation and evidence satisfy that standard. No open
finding remains in the selected scope.
