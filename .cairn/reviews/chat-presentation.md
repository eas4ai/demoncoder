# Chat presentation review

commitment: chat-presentation
commit: 9eda67d6889f298b872b0629e01cc8b06c1dfd68
findings:
  - none: no open findings within this commitment
Reviewer: Codex
Date: 2026-09-07
Status: complete

## What was examined

Reviewed the committed terminal event handling, activity block index, transcript
wrapping and retention, syntax parser state, tests, dependency settings and user
documentation without changing production code during this review. Implementation
commit bb1324c provides version 0.1.2.

Tool activity is updated from its original final receipt; streaming text is not
appended a second time. Cancellation labels unfinished activity as stopped rather
than successful. Compact previews count wrapped rows and retain six opening and
two closing rows. Full expansion uses the same retained text. Anchors identify
blocks and positions in logical lines, so wrapping and incoming output do not
move a reader to an unrelated row. The ordered indexes locate the visible slice.

Retention charges titles and text, limits logical entries, removes old blocks,
and trims oversized text at UTF-8 boundaries. Code colors retain parser state
between lines and reparse a changed streaming tail. Width changes reuse those
colors. Unknown syntax, oversized lines and code blocks fall back to plain text;
file-extension metadata is bounded. Terminal controls are made literal before
layout. The new render tests exercise Unicode, code colors, literal escapes,
outer gutter cells, compact/full scrollbar geometry and tiny terminal sizes.

## Failure demonstrations

The initial current-screen chat tests failed against the previous runtime:
long output had no compact preview or expansion hint, and source reads lacked
the requested operation heading and code colors. Both pass with the change.

A rendered-cell test then caught an actual scrollbar defect in the initial
implementation: at the latest output, its thumb ended at row 18 instead of the
track's final row 20. Inspection of Ratatui's implementation showed that its
content length counts possible viewport starts. Supplying total rows minus
viewport height plus one corrected the test. It also verifies the top position,
removal of the rail when compact content fits, and two blank outer columns.

The first expansion test also exposed a help line that hid the full-view state
while reading history. The corrected help reports both states. Existing
scrollback tests explicitly select full output now; their original mouse,
keyboard, resize, incoming-output and expiry assertions remain in place.

## Verification and delivery

Cairn receipt 20260907T014545261Z passes CHAT-001 through CHAT-004. Its mechanism
runs 23 library tests plus the production chat, scrollback and usage cases.
The additional checks on the final implementation also passed:

- cargo fmt --check; cargo clippy --locked --all-targets -- -D warnings;
  cargo test --locked --all-targets (55 passed, 3 intentionally ignored).
- scripts/check-startup.sh, scripts/check-developer-usability.sh,
  scripts/check-output-limits.sh, scripts/check-coding-session.sh and
  scripts/check-connections.sh.
- Fresh live two-turn disposable-repository checks for OpenAI API, Anthropic
  API, Codex and Claude, plus the live Oracle allow/deny verdict pair. The Oracle
  proposals were not executed. Their redacted records are committed.
- cargo install --path . --locked installed /home/shawn/.cargo/bin/demoncoder;
  --version reported 0.1.2. All six chat, scrolling and usage test methods passed
  with their binary reference set to that installed executable.

The first broad driver runs stopped at their live-evidence checks because the
inputs were still uncommitted. After committing and refreshing the live records,
both entire drivers passed. No freshness check was removed or weakened.

The ignored Cargo entry points are the explicit live Oracle case, independent
PTY adapter driver and selected-real-repository assessment. The first two run
through their separate drivers; this change did not run the harness against a
user repository.

## Limits and production self-audit

General Markdown prose and file links still retain their literal notation;
code highlighting is not a general Markdown renderer. Full output means retained
display content, not content already expired by the retention limit. Captured
backlog items cover Markdown rendering and the four source-review findings in
the supplied screenshot; this review does not claim to resolve those findings.

Reviewed the change against the production rules: scope, ownership, contracts,
errors, untrusted text, persistence boundaries, bounded work, meaningful checks,
documentation and truthful reporting. No further revision is needed within the
chat-presentation commitment. Provider execution, event evidence and permissions
were not changed by the presentation layer.

## Reconciliation review, 2026-09-07

Reviewed the current committed inputs at 9eda67d without changing application
code. Attacked whether the Creator prompt additions change event identity, tool
result grouping, scroll anchors, or usage formatting. They change request
instructions only; the terminal and transcript implementation is unchanged.
The completed/cancelled continuation and wrong-resume checks passed through all
four production adapters. `cargo test --locked` passed 55 tests, with 3 ignored;
`cargo build --locked` and `git diff --check` passed.

Cairn receipts at `.cairn/evidence/CHAT-001/20260907T062350676Z` and the matching
CHAT-002, CHAT-003 and CHAT-004 paths pass against the committed Creator change.
The mechanism ran library tests, build, chat-presentation, scrollback and usage
terminal drivers. Inspected the original receipt fields and retained output;
no live-provider refresh is claimed.

The existing usage driver deliberately expects `cost unknown` when another
usage dimension exists (`tests/usage.py:119`). That matches the current contract,
but the developer's later request changes it. Mouse dragging, native selection,
animation and the richer status strip are recovered work described in
`docs/recon.md`; passing CHAT evidence does not establish those features.

The installed spec lint reports eight pre-existing structure/path findings,
including CHAT-002's two obligations in one sentence. They are recorded in
`docs/recon.md` for the specification pass; this review does not silently change
Agreed wording or claim a passing lint. Existing source-review and Markdown
backlog findings remain open outside the four CHAT behaviors. No new behavioral
finding was found within the current commitment.
