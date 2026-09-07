# Usage display review

commitment: usage-display
commit: f36ca4bf88127c43a8decd1ae4a1386c34a7148c
findings:
  - none: no open findings within this commitment
Reviewer: Codex
Date: 2026-09-07
Status: complete

The initial baseline tested the previous contract and incorrectly appeared to
prove the new one. Review found its explicit usage-unknown expectation. The
corrected current-screen assertions failed on all four adapters against the
old runtime, showing usage unknown instead of the editor border immediately
above the controls. This was a development failure demonstration.

The correction clears usage on startup and each turn, suppresses all-None
records, and allocates zero rows when no usage text exists. Checking None rather
than numeric truth preserves reported zero. Partial usage retains unknown fields.
The event log and provider accounting are unchanged. Current-screen checks cover
startup, waiting, stale-value reset, known, partial, zero and absent usage across
all four adapters, and require scrolling controls to remain visible.

Cairn receipt 20260907T011313629Z passes the corrected usage checks and both
scrollback tests. Three output-limit terminal regressions and three screen-parser
tests pass. Formatting, all-target Clippy with warnings denied and diff whitespace
checks pass. The release was installed and the four-adapter usage check also
passes against /home/shawn/.cargo/bin/demoncoder.

The final review examined the small terminal/configuration-display diff and its
screen assertions without changing production code. No new dependencies,
provider behavior, persistence or permissions were introduced. README and the
selected specification agree with the behavior. The baseline finding is resolved;
no further revision is needed under the production rules for this correction.
