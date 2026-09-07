# Status and decision remediation review

commitment: status-decision-remediation
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
