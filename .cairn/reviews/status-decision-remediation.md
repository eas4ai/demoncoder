# Status and decision remediation review

commitment: status-decision-remediation
commit: d5c498158454960cc2dc2f2ef4ac098180baf5c5
findings:
  - REM-002: Historical agent checks remain inside role-input JSON rather than readable sections.
Status: in progress

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
