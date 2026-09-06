# Developer usability implementation review

commitment: developer-usability
commit: 2c75aeff96840a23873269fda1dc6cc8ba7a5983
findings:
  - none: no open findings within this commitment
Reviewer: Codex
Date: 2026-09-06
Status: complete
Open findings: none

## Evidence

The developer-usability mechanism passed USABLE-001 through USABLE-004 against
committed inputs, with final receipts at 20260906T195742549Z. Its checks exercise
production transcript layout and access enforcement, actual terminal scrolling,
and an assessment through native tools and controlled network endpoints.

The aggregate Rust test run passed 34 tests with two explicit live/driver entry
points ignored. The later explicit real-repository assessment compiled and passed
separately. The current developer-access suite passes ten ordinary tests with
that real-repository entry point ignored by default. Formatting, all-target
Clippy with warnings denied, and git diff --check passed.

The existing coding-session script passed its terminal, tool, responsiveness,
steering, cancellation, continuation, installed-backend boundary, result,
onboarding and host-access checks. Its final live-Oracle validation initially
rejected a stale receipt. A new verdict-only live allow/deny pair passed and its
validator now passes against current inputs; no proposed operation was executed.
Configuration, authentication, registry, capability-rejection, usage across all
four adapters, and six startup regressions also passed. This does not claim that
the separate four-provider live-connections suite was rerun.

Installed the production release with cargo install --path . --locked --force.
The installed /home/shawn/.cargo/bin/demoncoder passes both real-terminal
scrollback tests, including resize, input during streaming, history anchors,
retention expiry and mouse cleanup. Installed help and version also succeed.

The explicit read-only assessment of the selected DemonCoder repository is in
docs/reviews/self-repository-assessment.md. Reads, developer tools, Git, public
HTTPS and dependency audit worked through production default tools despite the
real build links and reference symlinks. The audit found no known vulnerabilities.
GitHub reported no Actions runs, which supplies no passing CI evidence. The
assessment correctly showed pending work and an unsynchronized branch at that
time; it did not present that snapshot as a finished commitment.

## What I challenged

- Display retention: empty deltas allocate no entries, fragments coalesce, byte
  and logical-line limits both evict, oversized text replaces its allocation,
  wrap-offset capacities stay bounded across resize, and expired history is
  visible. The bounds apply to retained display state, not provider context.
- Rendering and controls: cached idle layout scans zero bytes, visible-window
  iteration visits only requested rows and lines, incremental output reflows the
  tail, and absolute line/byte anchors survive width changes and arrivals. Tests
  cover Unicode streaming and more than 65,535 visual rows. Small terminal sizes
  do not place the cursor outside the editor.
- Access: outside reads resolve opened descriptors, native mutations stay rooted,
  ordinary internal hard links work, outside aliases stay read-only, and known
  credential stores and their aliases remain hidden. Git metadata can be changed
  through Git. Private caches cannot mutate host aliases and cold Cargo caches
  retain installed executable shims. Missing bubblewrap has no host fallback.
- Cancellation: the first review found synchronous directory traversal could
  defer cancellation before Bash. e889744 moves reads/preparation off the session
  task; dropping the await signals both traversals and cannot start a command.
  A safe variant with the signal disabled failed the deterministic test; the
  correction passes. A filesystem call blocked in the kernel is not itself
  interruptible by the cooperative flag, but cannot block the session task.
- Evidence: adding a scrolling-help row moved usage one row upward. The existing
  usage fixture failed until 9451a27 updated that row. All exact value, unknown,
  zero and reset checks still inspect the current screen and now pass. Controlled
  assessment checks preserve unavailable CI and stale-check failures.

Both recorded findings are resolved. The final review changed no production
code. This policy protects known credential paths and assumes another host
process does not maliciously replace entries during admission; it does not claim
universal secret discovery or a boundary against a hostile same-user host.

## Production self-audit

The implementation matches the selected chat and coding-access corrections.
Its mechanisms cover meaningful prior failures and safe violating cases. The
specification, decision records, README, tool descriptions, runtime and tests
agree. Display bounds do not claim durable recovery, model-context compaction or
cumulative budgets; those remain named later commitments. No further revision
is needed within the current commitment.
