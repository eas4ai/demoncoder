# Managed MCP runner prerequisite

Status: Bounded prerequisite implemented and approved by independent specification and quality reviews.

This prerequisite belongs to the complete skills-plugins-hooks commitment. It
does not establish public activation, full lifecycle delivery, connector
authentication or completion of the commitment.

Current production verification covers 200 library tests and all 33 Rust test
paths named by the dependency report: 23 MCP, 98 adjacent, and 238 remaining
cases passed, with sixteen explicit ignores. The final edit changed only the
MCP test observer; production bytes stayed fixed. Independent specification
review closed two reproduced write escapes and the traffic-observer defect.
Its unchanged adversarial harness passed four cases, including twenty
duplicate-response holds. Independent quality review accepted the implementation
and assessed the static findings; those tool results remain nonpasses. The
sections below retain the original findings and corrections.

## Source input semantics

Pinned Codex v0.153.4 implements MCP input substitution in
`codex-rs/hooks/src/engine/mcp_runner.rs`. Its recursive value substitution
preserves the JSON type of a sole placeholder. Embedded values become strings;
missing paths fail. Dotted lookup visits object fields, not array indices.
Keys remain literal and inserted event values are not expanded again.

The Claude Agent SDK 0.3.267 type declarations document `${path}` but do not
establish edge behavior. Three controlled runs of the actual Claude 2.1.267
executable used an isolated home, synthetic credentials, a local model endpoint
and a local stdio MCP server. One run requested a real Write operation; two
requested an MCP operation with typed arguments. Captured hook calls establish:

| Input case | Observed Claude substitution |
|---|---|
| Sole object, array, number or boolean placeholder | JSON text in a string |
| Null or missing field | Empty string |
| Literal non-string template value | Original JSON type |
| Nested template value | Expanded recursively |
| Template object key | Literal |
| Placeholder-looking event value | Inserted without another expansion |
| Array `.0` and `.length` | Element and length as strings |
| Array `.00`, string `.0` or string `.length` | Empty string |
| `${}` and `${tool_input.-}` | Literal text |
| Matching path with an empty segment | Empty string |
| Numbers `1.0`, `1e-7`, `9007199254740993` | `1`, `1e-7`, `9007199254740992` |

The final numeric case reflects JavaScript number rounding, also visible in the
actual model-requested tool call. It is not an authorization to change Native
or Codex numeric semantics. The Claude resolver uses the exact emitted host
event as its input; it cannot recover JSON key order discarded before that
boundary. Whole-object formatting must apply JavaScript integer-key ordering
to that event and preserve its remaining key order.

The three original captures are retained under
`/tmp/demoncoder-claude-mcp-input-f53smvyh`,
`/tmp/demoncoder-claude-mcp-input-0bagcx5s` and
`/tmp/demoncoder-claude-mcp-input-bb1jlci1`. Each retains the exact probe,
settings, MCP peer, model requests, MCP traffic, stdout and stderr.
`/tmp/plugin-claude-mcp-source-evidence-v2.json` lists 25 artifact hashes,
including executable SHA-256
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`.
The manifest SHA-256 is
`9e42fc22127c3eac90c3ce477e43d225cd1437465d6c2e1c462f45c729ae1f17`.
The parent verified all 25 hashes and asserted 21 extended placeholder fields
plus whole-object, embedded and nested correspondence to the original typed
call. These are controlled source observations, not DemonCoder runtime tests
or live-provider evidence. Temporary captures are diagnostic evidence; durable
production fixtures and their results remain required.

The extracted template, referenced event fields and exact observed arguments
are retained in `tests/fixtures/plugins/claude-mcp-input-source.json`. Its JSON
round trip and correspondence to the captured calls passed. This source
observation fixture retains the same evidence limit; production resolver tests
must account for the host's emitted event ordering.

`python3 tests/plugin_mcp_source_inputs.py --claude
/home/shawn/.local/share/claude/versions/2.1.267` passed against a fresh actual
CLI process. It compared all 25 fields, observed exactly two model requests and
two MCP calls, and rejected a deliberate change to the large-number result.
Its retained artifacts are in `/tmp/demoncoder-mcp-source-check-_xmk2o1t`;
all nine artifact hashes in `result.json` were verified. The harness checks the
executable pin, clears the environment, uses local synthetic endpoints and kills
its owned process group on exit. It qualifies source behavior only.
After changing executable hashing to a bounded streaming read, the same check
passed again in `/tmp/demoncoder-mcp-source-check-r9n7ct7p`; all nine captured
artifact hashes were verified against its result manifest.
An independent wrong-executable control passed: supplying `/usr/bin/python3`
was rejected by the pin check before peer files, settings or processes existed.

A separate controlled probe in
`/tmp/demoncoder-mcp-source-properties-ghefu64h` confirmed that Claude also
traverses inherited JavaScript object properties: `tool_input.__proto__` yields
`{}`, `tool_input.constructor` and `tool_input.toString` yield native-function
strings, and `tool_input.constructor.name` yields an empty string. This follows
the embedded resolver's object-only traversal with JavaScript property lookup.
These are not fields in the JSON event. The decision limits declared payload
access to JSON fields and requires an explicit pre-traffic failure for inherited
runtime properties. It does not silently substitute the missing-field result or
emulate native functions. An own JSON field with the same spelling remains
ordinary data and takes precedence. The final library run passes the resolver
unit control for this boundary; that result is separate from the controlled
source observations above.

The controlled run in `/tmp/demoncoder-mcp-source-protokey-vr3hvcfr` additionally
confirmed that a template object's `__proto__` key disappears from the emitted
arguments because Claude builds the result with JavaScript property assignment.
Source template keys that invoke this behavior require explicit rejection; this
does not prohibit reading an own JSON event field with the same spelling. Both
property probes retain their modified fixture and traffic; their artifact hashes
are listed in `/tmp/plugin-mcp-source-property-evidence.json`.

## Implementation choices

The decision record binds managed services to immutable authority and an
existing session/task owner. Individual calls still require current operation
admission. Reusing a connection cannot renew the original allowance. The first
explicit protocol target is MCP 2025-11-25; newer protocol strings do not imply
compatibility. Uncertain effectful calls cannot be automatically replayed.

Claude numeric formatting uses the focused `ryu-js` 1.0.3 dependency. The
registry reports Apache-2.0 OR BSL-1.0, Rust 1.71, and checksum
`04d056b875a9d2e6cb9a61d127afee9ac5999b9f87bcb32079d1318e505be714`.
No MCP SDK source is copied. The production-path and independent specification
checks below cover transport confinement, bounds, cancellation, secret handling,
schema stability and actual tool effects. Independent quality review is recorded
below.

## Initial independent specification review

The frozen candidate failed independent specification review. The report is
retained at `/tmp/demoncoder-mcp-review-evidence/initial-spec-review.md`
(SHA-256 `3adb86f053cd1aef12c0933d090b898973003499be17f1f97cffddbd208b1f87`).
The unchanged production-path probes admitted an actual write despite a missing
required output-schema field, and admitted writes in 20 of 20 operations when
the stdio peer supplied duplicate allow replies together in one write. Later
idle revocation did not protect the first operation. These are open defects,
not acceptable limits. Corrections follow as separate implementation work.

The review also requires durable MCP-specific discovery bounds, offline schema
validation, queued cancellation and stale-candidate/revalidation controls. Its
independent cursor-cycle hold and separate read-only revalidation controls
passed; the existing 12 repository MCP tests also passed while both defects
remained. The exact external harness and logs are bound by
`/tmp/mcp-spec-probe/frozen-manifest.json`
(SHA-256 `873bf9439cfad3340e61bf5e11a7ab8e5645b87e178ed17e41a186e776653d82`).
No quality approval or prerequisite completion is recorded.

The parent verified and copied all eight original harness artifacts to
`/tmp/demoncoder-mcp-review-evidence/spec-initial/manifest.json`
(SHA-256 `b9a36d90f021968987fb5af293b45460514517a82dcd7d3ebf50323ee6b6b54f`).
This preserves the initial failing logs when the reviewer reruns the unchanged
harness on the corrected candidate.

## Prerequisite verification obligations

These checks are required before this bounded prerequisite can be marked
complete. They do not replace later public service/lifecycle verification.

| Contract | Required observation | Initial review result |
|---|---|---|
| PLUG-003 decision validation | Missing or invalid schema-bound structured output holds the actual write; valid output permits it; both transports | Open: missing required output field permitted a write |
| PLUG-003 invalid replies | Duplicate replies already delivered together hold the first actual write; one reply permits it | Open: 20 of 20 duplicate cases wrote |
| PLUG-002 managed admission | Offline schema and allowed-tool refusal precede forbidden call traffic; no unadvertised service capability gains host authority | Focused durable coverage required |
| PLUG-011 bounded discovery | Complete multi-page catalog succeeds; cursor cycles and declared resource limits hold without tools/call | External cursor-cycle negative passed; durable bounds matrix required |
| PLUG-002 cancellation | A call cancelled while waiting for the connection never emits a later request | Focused durable coverage required |
| PRUN-001 final candidate | Changed candidate cannot reuse primary approval; separate admitted read-only check can approve without primary replay | External positive passed; durable positive and negative required |
| PRUN-002 freshness | Required atomic external precondition holds when the operation cannot supply it | Focused MCP regression required |
| PRUN-002 original allocation | Reuse does not renew the original service owner deadline or allowance | Service-level coverage assessment required |
| PLUG-011 service ownership | Complete identity changes cannot reuse old authority; failed startup/shutdown obey resource bounds | Existing cases need explicit mapping and gap assessment |

Bootstrap preparation precedes durable hook invocation reservation. The initial
implementation acquires a transient whole-group concurrency permit before
preparation; that permit is a resource bound, not a durable invocation receipt.
The review must verify startup ownership and avoid claiming that preparation
precedes every kind of reservation.

### Correction evidence discipline

The first new output-schema test log
`/tmp/demoncoder-mcp-review-evidence/correction-early/mcp-output-schema-red.log`
failed its valid-response control: adding a top-level `proof` field also
violated the native source-result format. It is a fixture/debug failure, not
a meaningful reproduction of missing MCP output validation. The original
independent missing-proof actual-write reproduction remains valid. The corrected
positive fixture must constrain a field already allowed by the source format;
a validator-removal control must demonstrate an actual invalid-response write.
The early coalesced duplicate test did demonstrate an actual first write and
its corrected test passed; these preliminary results still await independent
review of the final frozen candidate.

The corrected output-schema deletion probe did fail for the intended reason:
with validation removed, `output_missing` returned a successful actual write.
After restoration, four selected schema tests passed, including the both-transport
valid/missing/wrong/text-only matrix. The discovery-boundary matrix and queued
cancellation test also passed. The parent read those logs and retained copies
at `/tmp/demoncoder-mcp-review-evidence/correction-controls/manifest.json`
(SHA-256 `196589a079fbf664daa5d71ba1919e6376520d11f6d4572874fe0b20a9a33651`).
The exact original and mutant output-validation source hashes matched the
mutation record; copies are bound by
`/tmp/demoncoder-mcp-review-evidence/output-mutation/manifest.json`
(SHA-256 `4f9b5d2434c9e658bbed9fd9a2f368b593419a1c34a0a4c73e551706d9268e28`).
These are intermediate correction checks, not approval of a final candidate.

### Test environment recovery

The first service-authority boundary run exhausted `/tmp` inodes while creating
fixtures. Its failed setup is retained as
`/tmp/mcp-service-authority-boundaries.log`; it is not a semantic test result.
`df -i` showed only three free inodes although 17 GiB of byte capacity remained.
The existing backlog already records the legacy empty confinement-probe cleanup
problem. The parent removed 1,000 old, UID-owned, unreferenced probe roots with
the exact known empty directory layout, using `rmdir` only. No files were
deleted; 15,000 inodes were freed. Process command lines, mounts and descriptor
paths contained no references to those roots. No runtime cleanup code changed.

Reruns used `TMPDIR=/home/shawn/.cache/demoncoder-mcp-test-tmp`. The parent read
the retained `authority-rerun.log` (seven passed), `startup-release.log` (one
passed) and `coalesced-extended.log` (one passed). The startup case explicitly
reconciles the known fixture failure after observed teardown, then starts eight
services to check capacity release; it does not imply automatic reconciliation
of an unknown remote effect. Final candidate checks and review remain required.

## Corrected candidate re-review

The corrected 26-file manifest at
`/home/shawn/.cache/demoncoder-mcp-test-tmp/corrected-frozen.sha256`
(SHA-256 `0e573b1fe03e2c07dccc30389cf91e0f188bd307a221ea5d034db1c7cdd94859`)
matched every source file. The parent retained a reconstructable overlay against
`b22487cfb359f212a61bcd4156979211a4ae48f2` at
`/tmp/demoncoder-mcp-review-evidence/corrected-source/manifest.json`
(SHA-256 `6e27f7d8016d4d7751567a69816561939535362d51d3cd6a5d899f16638cc7d3`).
The parent verified the final logs contain 200 library and 22 MCP passes,
98 adjacent passes, and a further focused external-reference test pass; Clippy
completed successfully. Only the external-reference test was strengthened after
the 222-test run; no production code changed.

The independent reviewer reran the unchanged original four-test harness: all
four now pass, including the original invalid-output hold and all 20 coalesced
duplicate first-write holds. This closes those reproduced behavior failures on
the reviewed candidate, subject to the completed review.

A residual test-observation finding remains open: the external-reference test
asserts `peer.methods().is_empty()`, but that helper omits requests without a
JSON RPC method. An HTTP GET for a schema is therefore invisible to the
assertion. The test must inspect all captured requests and demonstrate that an
injected GET falsifies the zero-traffic observation. The reviewer is finishing
read-only inspection before that separate test correction begins.

The completed re-review independently passed all 22 repository MCP tests and
confirmed the observer defect with a real local GET: one raw request was captured
while the RPC-method list stayed empty. The full report is retained at
`/tmp/demoncoder-mcp-review-evidence/corrected-spec-review.md`
(SHA-256 `5a812bdb833a46e584d1af2975d59ab59ebbb8742a68779e3c08ecae36c33d76`). Its new evidence
manifest is `/tmp/mcp-spec-probe/rereview-manifest.json`
(SHA-256 `accad8b661a081fa867fe286c9d62b6376c43d3e116cff684b92acb84f6fe3de`).
Both P1 findings are closed by independent execution. Specification approval
is withheld until the P2 observation correction is separately verified.

The parent then ran the 28 remaining Rust targets named by Ripwire's test gate,
excluding the already exercised MCP and four adjacent suites. With normal
`TMPDIR=/tmp` and one test thread, the run exited zero: **238 passed, 16 ignored**.
The captured command and every result are retained in
`/tmp/demoncoder-mcp-review-evidence/parent-remaining-regressions.log`
(SHA-256 `749e9c9ae67345d3a9fcfc91d0ba807059be8941f783fcbf5ffef6f73a875ff7`).
The production source stayed frozen; only the separately owned MCP test observer
was being corrected, and that test target was not part of this run.

The ignored entries require installed Rust/TypeScript services, selected live
Oracle or public assessment, installed Claude/Codex barriers, inherited
descriptors, or PTY/subprocess drivers. Their registration is not execution.
Earlier separately driven evidence keeps its own candidate and limits; this
run does not claim new installed/live qualification. Together with the worker's
200 library, 22 MCP and 98 adjacent passes, all 33 Rust test paths named by this
static report have executed their default cases. The static test-gate exit four
remains a nonpass; its unmodelled edges still require review.

## Final specification approval

The final 26-file manifest is
`/home/shawn/.cache/demoncoder-mcp-test-tmp/observer-frozen.sha256`
(SHA-256 `0cd180a92c7ebe7f1126815ee3d521f5ef269bc63a884fd61ca8c438fc947272`).
The parent verified every entry and that only the MCP test file changed from
the preceding reviewed candidate. Six observer correction artifacts, including
the exact old-observer mutant, meaningful failing log and 23-test passing run,
were verified and retained at
`/tmp/demoncoder-mcp-review-evidence/observer-correction/manifest.json`
(SHA-256 `37fb373428c18f205abcabe3b50dc826ffe67bafba726929dfa2786d5708e83c`).

The independent reviewer approved the bounded prerequisite after independently
passing the GET-observation and external-reference rejection tests. Both P1
production defects and the P2 coverage/observer finding are closed. The full
report is retained at `/tmp/demoncoder-mcp-review-evidence/final-spec-review.md`
(SHA-256 `87b2433535d9175c01cafa23e172ba1109e3b73037f89478a084c6a0155d2cef`). It carries forward the
unchanged-production four external passes and 22-test re-review without claiming
they were rerun after the test-only correction. Quality review remains pending.

The parent refreshed Ripwire on the final test correction: quality-delta exited
2 (52 gating findings, 105 new-symbol findings); test-gate exited 4 (33 test
paths, 186 unmodelled symbols). Reports are
`/home/shawn/.cache/demoncoder-mcp-test-tmp/observer-quality-delta.txt` and
`observer-test-gate.txt`. These are nonpasses awaiting independent disposition,
not waived results or runtime defect counts.

## Final quality approval and prerequisite audit

The independent quality reviewer found no critical or important defect and
approved this bounded prerequisite. Its own runs passed 23 MCP integration, two
protocol and four source-input unit tests. All 26 source hashes matched before
and after review. The parent verified and copied all three logs and the full
report to `/tmp/demoncoder-mcp-review-evidence/quality/manifest.json`
(SHA-256 `2d08dad1db799ca1481d4f7a9d0d98860fa4b146ddd4e5083fcb8810aa56ee7d`).

The review assessed actual protocol and ownership complexity, small bounded
writer duplication, fixture scaffolding, recent-change warnings, and name-based
attribution to unchanged `reflects` and `run` functions. It found no concealed
success-enabling failure or unbounded retry. It accepted the required preparation
and durable reservation ordering without splitting state transitions merely to
reduce line counts. The quality-delta and test-gate exits remain 2 and 4; neither
is reported as a passing result. Their wider static uncertainty does not replace
the executed production tests or later complete-commitment checks.

Two nonblocking maintenance recommendations are captured in the backlog: measure
repeated schema-compilation cost before retaining compiled validators, and
consider small shared fixture-ownership utilities when another runner needs them.
Neither was implemented as unrelated cleanup.

The parent reviewed this prerequisite against the production standard: authority
and failure boundaries are explicit, changes reuse existing confinement and
accounting, source semantics and dependency provenance are retained, and executed
checks plus independent reviews support its bounded behavior. No open finding
remains for this prerequisite. This is not full delivery: public activation,
configuration/OAuth, ordinary model-facing managed tools, dependency graph and
full lifecycle integration, channels, migrations, and live connector qualification
remain required under the same selected commitment. Formal Cairn evidence is
recorded only when its complete mechanism is ready on a committed candidate.
