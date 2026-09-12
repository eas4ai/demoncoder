# Native turn identity and imported hook framing

Status: specification and quality reviews approved the frozen candidate.

This stage builds the [native turn decision](../decisions/give-each-native-turn-a-durable-identity-before-hook-selection.md)
on the [reviewed native lifecycle foundation](plugin-non-tool-runtime.md).
It remains part of the complete skills/plugins/hooks commitment.

## Behavior and ownership

A real native turn has a durable record before optional hook selection. Submit,
Stop and internal corrections share its identity. Stop-only configurations and
plugin-origin work have real turns without a fabricated developer submission or
a second allowance. Existing operation ownership, recovery and cancellation
remain authoritative.

Imported Claude and Codex hook formats receive genuine native host facts.
Translation provenance belongs in host records; closed source schemas receive
only their declared fields. Actual assistant text comes from the current model
response, not tool, advisory or plugin output. Capture is bounded and enabled
only for configured Stop hooks. Overflow preserves original transcript/UI output
and visibly holds Stop framing instead of substituting null or truncated text.
Missing required Codex model facts hold before command or model I/O.

The native transcript_path refers to the actual host Store state.json, which
retains session messages. It is host session evidence, not Claude JSONL or a
Codex rollout file. Child paths refer to that owning host session evidence;
this stage does not claim a separate agent_transcript_path or SubagentStop
integration. Package workflows that read source-specific transcript formats
still need complete compatibility validation.

Existing NativeSession callers may have no workflow phase, with or without an
active task. Their records retain those facts without manufacturing a phase.
Such a record cannot authorize lifecycle hooks or gain authority from a later
phase. Concurrent bare sessions retain distinct IDs and the existing shared
mutation and allowance controls. Active workflow exclusivity is scoped to the
exact owner. Individual Submit/Stop occurrences and their reservations remain
distinct even when they share a turn.

## Failure demonstrations and repairs

Retained logs under /home/shawn/demoncoder-check-tmp/ show three actual-loop
missing-record failures before implementation, followed by those three cases
passing. The imported native command case also failed before framing and passed
afterward for both dialects. The final command tests exercise two actual turns
per dialect, exact schema-valid fields and assistant text, shared IDs within a
turn, distinct IDs between turns and host-only provenance.

Broader tests exposed positional operation assumptions and existing no-phase
callers. Diagnostic logs with green in their names still contain failures:
native-turn-owner-green.log records four passed and one failed;
native-turn-lib-green.log records 283 passed and one failed. They must not be
reported as successful commands. The reload fixture was corrected to read the
Store envelope through Store.read; a separate actual reopen test verifies
uncertain-turn recovery.

Additional failing controls exposed initial-submission authority borrowed from
a plugin-origin turn, a text-capture limit affecting bare sessions without Stop,
and missing retained evidence when configured Stop capture overflowed. These
were repaired, including legitimate developer steering during plugin-origin
work. A later integration case exposed the concurrent bare-session regression;
its original mutation-admission assertions now pass without alteration.

Subsequent repairs retained the new turn linkage in the shared reserved-hook
view and preserved observer cleanup when turn admission fails. Cross-Stop
reservation/outcome refusal prevents the shared turn identity from granting
authority over another occurrence. All final checks below ran after those
production repairs.

## Final candidate verification

The 191-file Rust aggregate is
`862453f956ea6111268889a765d8ddb984278a4cc1d58d9ab9db19d6f911009e`.
The parent independently reproduced it. The established recipe sorts relative
Path objects under src/tests and hashes path, NUL, bytes and NUL. Sorting path
strings instead produces the separately recorded alternate digest
`de08640c144b17410d0d82f35b571750cf8d332d820256f963ea23762b96cf9d`
for identical bytes; it is not another candidate.

| Check | Result |
| --- | --- |
| Locked library | 289 passed |
| Admission | 28 passed |
| Command / HTTP / MCP / model runners | 33 / 26 / 26 / 18 passed |
| Once / post-tool / tool receipts | 19 / 48 / three passed |
| Locked all-target Clippy, formatting, diff check | Passed |
| Ripwire edit check | Exit zero |
| Ripwire quality delta / test gate | Exits 2 / 4; diagnostics retained |

The eight integration targets total 201 passing tests. Exact commands and logs
are recorded in native-turn-identity-handoff.md; native-turn-lib-final-1.log and
native-turn-runners-final-1.log contain the final test results. No complete
all-target test run or Cairn evidence is claimed from this stage.

Static analysis retains 26 gating quality findings and broad coverage
obligations. It is not a passing static gate. Independent specification review
approved this exact candidate and reran 11 native-turn tests, two source command
tests and 15 overlapping lifecycle tests; all passed. Quality review separately
passed 11 turn tests, 15 overlapping lifecycle tests, two source tests and one
model-admission test, and approved the unchanged candidate with no required
findings. These selections are not unique-test totals. Existing dispatcher
complexity and correction-drain duplication remain nonblocking concerns.

The final self-audit checked the bounded implementation, ownership and allowance
contracts, compatibility, failure/recovery behavior, verification and documentation
against the production rules. It found no additional required repair for this
stage. The complete commitment and release remain open.

## Remaining scope

Actual authenticated external Submit/Stop relay integration remains subsequent
work. Other lifecycle events, explicit child service provisioning, component
activation, management and complete installed/live conformance remain required.
This stage does not establish the complete commitment or release readiness.
