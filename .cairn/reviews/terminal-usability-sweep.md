# Terminal usability sweep review

Status: in progress

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

Implementation and verification remain pending. Record failing demonstrations
and passing corrections below as each mechanism is built.

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

Final committed checks, installation and no-code review remain to be recorded.
