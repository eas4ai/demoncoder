# Status and decision remediation review

commitment: status-decision-remediation
commit: 4dfdb6c10af586793c942a04ff4caaad0a4180c8
examined:
  - Authoritative runtime projection, stale/error state and lossy notices.
  - Bounded Unicode paging, role/check history and immutable original evidence.
  - Terminal draft/history preservation, cancellation and restart.
  - Command authority, current-file gates, documentation and installed release.
findings:
  - resolved: REM-002: Historical agent checks now render from retained role inputs; the failing multi-round regression and production inspection cases pass.
Status: complete

## Baseline failure demonstration

The new production REM-004 case ran on the pre-remediation debug binary and
failed at opening inspection with F2 while a provider was held. The prompt
retained draft-λ and historical ROW lines, but no Inspection view appeared.
This establishes missing behavior, not a passing mechanism. The fixture closed
the process and released its provider in finally cleanup.

Source recon also found literal agents 0 at src/terminal.rs:773, while actual
counts are delivered only as transcript notices. New checks will compare the
visible strip with actual durable running and queued child records.

The REM-001 production fixture also ran against the installed pre-remediation
release. It observed a running child and queued dependent in the durable record,
but the rendered strip still said agents 0, so the assertion failed as intended.
The corrected build passes that same comparison and the cancellation transition.

The original ORCH-006, ORCH-007 and SUB-007 specification files from 3cdf445
failed installed spec lint for their compound obligations. Splitting the
independent obligations leaves their required behavior and falsifiers intact;
the corrected specification passes the installed lint.

## Implementation checks

Production REM-001 through REM-004 fixtures passed during implementation. They
exercise actual retained counts, child and task evidence, large Unicode pages,
stale-file acceptance refusal, read-only navigation, preserved draft/history,
cancellation, and interrupted restart without replay. Formatter tests retain
original evidence and quote role text as data. Runtime tests force advisory
notice loss and lock contention and reject failed persistence as current state.

A read-through found that a summary refresh could overwrite an unread page in
the watch slot. The background reader now retains the single requested page;
a regression test waits for consecutive publications and checks both carry it.
The terminal also tests late-generation rejection and unknown/aged/error state.

Ripwire was scoped to src after its repository-wide run included unrelated
ignored reference checkouts. The source quality delta has zero gating findings.
New-symbol report sizes reflect labeled evidence rendering and command cases;
key dispatch retains existing terminal conventions. Its dead-code findings
include tested methods/types whose receiver edges are not resolved. Edit-check
identifies Inspection as a new symbol with no incompatible callers found.
The source test-gate exits 4 and cannot map the external Python PTY drivers;
Cargo and the declared production mechanisms supply the actual verification.

## Revised mechanism review: SUB-007

Compared 3cdf445..HEAD specification text, the assignable-subagents declaration,
its shell runner, and recovery/interrupted_transition in the production driver.
The edit splits uncertainty and no-replay obligations without changing either,
identity, original results, allocation, or message authority. The driver kills
actual work and preparation/integration intent; restart assertions check uncertain
state, retained identity/allocation, no new provider request, and unchanged files.
The integration case also retains original passing checks and review evidence.
No mismatch or missing declared input was found.

Safe violating observation: wrapped only the test reader's returned dictionary
to substitute integrated for uncertain after a real restart. SUB-007 rejected
that fabricated success at its status assertion. The durable record and runtime
were unchanged. Then the unmodified SUB-007 production driver passed all four
transport recovery cases and interrupted preparation/integration cases. This
demonstrates the relevant assertion, not arbitrary corruption detection.

## Revised mechanism review: ORCH-006

Compared the unchanged shell runner and declaration with the revised individual
and parent cancellation sentences and their existing falsifier. Examined
cancellation and cancellation_persistence_failure: all four transports exercise
held advisor/worker-response/judge activity, independent parent input, attributed
state events, individual cancellation, parent cancellation and shutdown. The
cases check the two-second bound, stopped heartbeat effects, peer shutdown, and
no queued worktree creation after parent cancellation. The selected-only case
allows independent queued work to finish. The new REM inspector checks supplement
these assertions with rendered evidence, command guidance and input preservation.
No mismatch was found; the sentence split removes no obligation.

Safe violating observation: in the persistence-failure case, a temporary wrapper
appended synthetic bytes only to the second heartbeat read returned to the test.
The unchanged assertion rejected continued effects after cancellation. No file
was altered by the wrapper. Then the complete unmodified ORCH-006 production
cases passed, including cancellation when durable transition persistence fails.

## Revised mechanism review: ORCH-007

Reviewed the sentence split against recovery, recovery_authority_and_integration
and recovery_limits. Uncertainty and no automatic replay remain separate required
behaviors; the original identity, evidence, correction and allowance obligations
are unchanged. The only declaration change so far is the ORCH-006 reviewed entry,
not its command or inputs. Existing cases kill four transport types in each role
and correction phase, retain dependency/receipt/counter identities, assert no HTTP
or backend restart and no queued work, refuse changed authority, and preserve
spent allowances. Explicit inspection alone cannot release work. No mismatch.

A temporary test-reader wrapper substituted integrated for uncertain after an
actual interrupted role restart. The recovery assertion rejected this fabricated
prerequisite success; the runtime and stored evidence were unchanged. The
unmodified complete ORCH-007 driver then passed recovery, authority/integration
and retained-limit cases. This is a bounded assertion demonstration, not a claim
that a deliberately corrupted runtime was deployed.

## Final candidate finding

Read the complete formatter, terminal/poller handoff, command wrappers and
production cases against REM-001..005. The runtime replaces agent.checks for
each correction generation and retains earlier check receipts inside each role
receipt evidence. agent_report renders current checks and all role conclusions,
but its historical role inputs remain JSON-only. Earlier command output therefore
requires decoding JSON, which does not meet REM-002 for readable original checks.
Record this before a separate repair. Render those retained check generations
with the existing labeled check formatter and add a multi-round regression that
requires real newlines in historical output. Do not change stored evidence.

### Historical agent check repair

The new multi-round regression failed on f607b1e because readable historical
check sections were absent. It now reconstructs three correction generations
across bounded Unicode pages, checks original multiline output, failed/passing
exit statuses, and byte-identical stored evidence. The formatter reads only the
top-level checks field from each retained role input and uses the existing check
renderer; nested role inputs are ignored during this projection. Unreadable or
missing check lists have an explicit fallback and keep the original input.

All 13 targeted inspection tests and all 73 library tests pass. The full
remediation driver passes REM-001..005 after the repair. Strict all-target
Clippy and diff whitespace checks pass. Ripwire reports no gating quality
regression and unchanged agent_report signature. The test gate's source-only
reachability gaps remain supplemented by actual Cargo and production PTY checks.
Final committed evidence refresh and installed release verification remain due.

## Final review of the repaired candidate

Reviewed the committed historical-check repair against the full inspector,
runtime projection, command wrappers, terminal input flow and declared contract.
The original finding is resolved: every retained role input projects its original
check list into labeled output, including earlier correction rounds. Parsing
failure remains explicit and cannot turn missing checks into a pass. The
read-only renderer does not change original receipts or grant command authority.
No additional in-scope finding remains. No code changed during this final review.

The page boundary tests reconstruct Unicode and multi-round output without gaps.
The reader uses try_lock off the input path, retains only the requested page,
and rejects late request generations. A cached page may be historical by design;
the display identifies it as saved evidence and says files were not rechecked.
The existing acceptance/integration commands still check actual files. The
production cases cover stale-file refusal, read-only navigation, draft/history
retention, narrow resize, cancellation and restart without replay.

All ten named commitment requirements have current passing Cairn receipts after
7c41d14. The shared mechanisms also passed all seven subagent requirements, all
seven orchestration requirements, and the inherited terminal checks. The status
mechanism ran formatting, strict all-target Clippy, the full all-target Rust
suite, and existing status/usage/continuation/registry/startup terminal drivers.
The library has 73 passing tests; fixture/live entry points explicitly ignored
by the general Cargo command are not claimed as executed by that command.

### Installed release verification

Ran cargo install --path . --locked from this candidate. The installed executable
at /home/shawn/.cargo/bin/demoncoder and target/release/demoncoder match SHA-256
39060995caa1d9a366341d6ccf9175aa3817d08644bbc8e087ceec26b189bfba.
With DEMONCODER_TEST_BINARY selecting that installed executable, production
REM-001, REM-002, REM-003 and REM-004 all passed. These use controlled provider
fixtures with real terminal, process and filesystem behavior. No current live
external-provider verification is claimed.

### Production standard self-audit

1. Traced the existing task, agent and terminal owners against the agreed scope.
2. Kept the change within prerequisite presentation and documentation repairs.
3. Reused runtime records and check rendering; added no durable store or daemon.
4. Preserved existing public terminal entry points and durable record formats.
5. Made unavailable state and parsing failures explicit; identity display omits credentials.
6. Kept quoted evidence separate from controls and retained existing effect gates.
7. Inspection performs no persistence; interrupted work still needs explicit reconciliation.
8. Bounded paging and background refresh; tested contention, cancellation and stale publication.
9. Maintained one action in progress and resolved the finding only after repair verification.
10. Ran failure demonstrations, production checks, Rust checks and installed-binary cases.
11. Limited claims to executed evidence and documented saved-file freshness limits.
12. Applied the developer's prerequisite scope and decision to continue past malformed review metadata.
13. Re-reviewed the repaired candidate; no further revision is indicated by this audit.
14. Documented controls, consequences and limits in plain language.

The earlier unprefixed finding was corrected to open before work resumed. Its
current resolved entry records an actual verified repair, not the previous
parser-induced clean result. Cairn validation remediation is separate developer
work and is not claimed as part of this repository's change.
