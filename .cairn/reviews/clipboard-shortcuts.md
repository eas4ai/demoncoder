# Clipboard shortcut review

commitment: clipboard-shortcuts
commit: 3f1aff527119b9d86a6a07b7d59f7f6e4b171bc2
findings:
  - none: no open findings within this commitment
Reviewer: Codex
Date: 2026-09-07
Status: complete

## Initial mechanism finding

Initial mechanism review found that the inherited interaction tests did not
exercise prompt copying or bracketed-paste mode. Its first passing receipts
therefore do not establish the new requirements. Added focused prompt-only PTY
checks to the mechanism; both fail on the previous runtime. Copy emits no OSC 52
request, and bracketed paste is not enabled. Implement the prompt shortcuts and
mode lifecycle, preserving the separately tested transcript Ctrl-Y behavior.

## Resolution and final review

Reviewed the committed terminal branches, mode guard, prompt titles, README and
CLIP requirements without changing production code. The copy branch reads only
the bounded prompt input. It accepts distinguishable Ctrl+Shift+C before the plain
Ctrl+C cancellation branch; an empty prompt does neither. Transcript Ctrl-Y remains
separate. The terminal supplies Ctrl+Shift+V clipboard contents through paste;
DemonCoder does not query the OS clipboard. Bracketed paste strips controls and
newlines, respects the existing 64 KiB editor bound and does not submit a turn.
The input guard enables and restores bracketed paste and enhanced key reporting.

The focused PTY checks prove Unicode prompt copying while a turn is active,
empty-copy isolation from cancellation, no automatic clipboard request, safe
control/newline paste, no prompt submission, and restoration of paste mode on
exit. The inherited false-positive mechanism was strengthened before accepting
completion. A subsequent Clippy guard suggestion was fixed with an explicit
exclusion of Shift from cancellation; the new empty-prompt case protects that
boundary. All prior receipts remain retained as historical observations.

Current committed evidence passes both requirements: formatting, Clippy with
warnings denied, 34 library tests, two prompt clipboard PTY checks, four transcript
interaction regressions, provider/tool cancellation and continuation on all four
adapters, and specification lint. Ripwire edit-check found no incompatible caller;
quality-delta passed with one minor verbosity observation. Its test-gate exits 4
because it does not model the Rust/PTY tests; executed tests supply that evidence.

The release was installed with cargo install --path . --locked. Its hash matches
target/release/demoncoder and PATH resolves to the installed file. Both focused
prompt PTY tests passed against the installed binary; path, hash and results are
in .cairn/evidence/clipboard-shortcuts-install.log.

Terminal-reserved shortcuts remain a documented capability limit: copying the
application prompt needs Ctrl+Shift+C forwarded with distinguishable modifiers
and terminal OSC 52 support. The tests verify that input/output protocol, not a
particular desktop terminal's bindings or system clipboard. The developer's
prompt-only scope is preserved. Final self-audit against the production coding
standard found no open scope, correctness, security, verification or documentation
issue requiring revision in this commitment.
