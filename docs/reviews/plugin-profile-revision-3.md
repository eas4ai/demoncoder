# Codex SessionEnd MCP profile correction

Scope: the developer-approved correction in
`.cairn/escalations/pcomp-002-pcomp-004.md`, answered `ok` on 2026-09-11.
This is a prerequisite correction, not completion of the plugin commitment.

## Change and source evidence

Profile v1 revision 3 changes exactly one of 510 resolved applicability cells:
Codex SessionEnd/mcp_tool is source-nonexecuting. Command shutdown handlers,
neighboring Codex MCP events, Claude MCP shutdown and native MCP shutdown retain
Run. The embedded loader pins revision 3. Explicit native conversion and actual
shutdown execution remain required full-commitment work.

The pinned source and its hash are recorded in
[the contract](../spec/plugin-compatibility.md#profile-v1-revision-3-correction).
Original source retrieval and byte comparison ran. The upstream negative test
was inspected, not executed.

## Verification and failure demonstrations

The new profile regression failed against the original Run cell, then passed
with SourceNonexecuting. The independent Rust wire oracle likewise rejected the
changed cell until its expected source behavior was corrected. Two independent
result-decoder cases exposed the same old expectation; their shared source
support oracle now explicitly excludes this pair. The exhaustive decoder case
still sends its response and requires rejection, rather than skipping it.

Executed checks on the final production code:

- 227 library, 1 capability, 28 admission, 30 command, 24 HTTP, 23 MCP, 16 model,
  6 Codex post-tool and 48 post-tool tests passed.
- 49 result tests and 11 wire tests passed after correcting their independent
  source expectations. The registry terminal entry was explicitly ignored by
  Cargo; it was not an executed terminal check.
- The inventory semantic checker passed with Ajv 8.17.1: all 510 cells, the
  missing-cell mutation, all model branches, 14 outcomes and four schemas.
- An independent before/after expansion confirmed exactly the approved one-cell
  semantic change. The remaining inventory was preserved.
- All-target Clippy passed. After the final result-test edit, its targeted Clippy
  check passed. Formatting and whitespace checks passed.
- Ripwire edit-check found no signature change. Quality-delta exited 0, reporting
  the new Rust test as statically dead and a small independent-oracle length
  increase. The test actually ran; the extra branch encodes the required exception.
  The initial test-gate exited 4 with 13 test files and 30 unmapped symbols;
  its named Rust suites ran as listed above. After editing the test-only
  `supported` helper, its name-based graph expanded to 37 test files and 514
  unmapped symbols, including unrelated application paths. This final static
  result is not a pass. The sole production edit remains the embedded revision
  pin; the changed test helper has no production callers. Its complete result
  suite was rerun rather than treating that static expansion as new runtime code.

These are development checks, not Cairn receipts or installed/live-provider
qualification. No historical evidence receipt was changed.

## Independent review

Specification and quality reviews approved all eight implementation/specification
files, including the final independent result-oracle correction. Both verified
stable reviewed file hashes and HEAD. Neither claimed to run the parent-owned
Cargo checks. No Critical or Important finding remains in this bounded correction.

## Production self-audit

The change is limited to the approved contract correction and its loader pin,
source expectations and regression checks. No dependency, public interface,
credential handling, durable migration or execution authority was added.
The failing examples establish that the tests distinguish the previous behavior.
Full runtime lifecycle, activation, native conversion and live conformance remain
on the existing plan; none is claimed delivered here. The production rules were
reviewed and no unresolved issue in this bounded correction was found.
