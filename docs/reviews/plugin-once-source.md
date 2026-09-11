# Pinned Claude one-shot source qualification

This fixture observes Claude Code 2.1.267 through its actual SDK transport,
with project skill loading and a synthetic local model peer. It does not
exercise DemonCoder or a live provider. The executable must match SHA256
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`.

Run from the repository root with a new output directory:

```sh
python3 tests/plugin_once_source_inputs.py \
  --claude /path/to/claude-2.1.267 \
  --output /path/to/new-qualification-directory
```

The fixture uses an isolated home and configuration directory and a synthetic
API key. It inherits no ambient credentials. Each case makes five model requests,
three actual SDK MCP tool calls and two successful turns in the same session.
The first turn invokes the skill and makes tool calls 1 and 2. The second turn
makes call 4. A control leaves the skill unloaded.

| Case | Hook calls observed |
|---|---|
| `once: false`, success, explicit reinvocation | 1, 2, 4 |
| `once: true`, success, explicit reinvocation | 1, 4 |
| `once: true`, exit 1, explicit reinvocation | 1, 2, 4 |
| Async exit 1, second call after completion marker | 1, 4 |
| Async exit 1, second call before completion marker | 1, 4 |
| Success followed by an ordinary prompt | 1 |
| Skill never invoked | None |

The verifier checks the loaded skill marker in real model requests, actual
invocation prompts, SDK calls, same-session turn results, hook event identity,
original tool output and command start/end records. Forty-eight corrupted-evidence
controls must fail. `qualification.json` records executable, fixture, harness and
artifact hashes; raw events, model requests, generated commands and traces remain
beside it. The fixture requires a fresh output directory and rejects a wrong
executable hash.

Development verification passed all seven cases and 48 corruption controls,
including an output path containing spaces. Ruff and Black passed. Independent
specification and quality reviews approved this three-file source fixture.
Both reverified the retained cases, controls and all 69 artifact hashes. The
quality review also exercised malformed-output cleanup with a synthetic peer;
the process was reaped and the server thread closed in 0.50 seconds.
This is not a Cairn receipt or approval of the production runtime.

Review found that the verifier could trust a summary after losing its raw command
log and could accept an SDK call with the wrong tool name. It now compares the raw
log with the summary and validates server, tool and operation identity. A separate
refusal check found that an assertion cannot protect against Python's `-O` mode;
an unconditional runtime check now rejects it before launching any process.
Wrong executable pins also fail before creating the output directory.

The completion marker is emitted immediately before deterministic `sys.exit(0)`
or `sys.exit(1)`. Independent OS exit-status telemetry is not retained. Async
timing establishes order relative to that marker, not the source's exact internal
callback timing. Restart/resume is not tested, and only explicit same-session
slash invocation tests reactivation.

Claude's async one-shot behavior consumes at launch. DemonCoder's agreed contract
requires actual successful completion before consumption. The implementation must
retain this deliberate difference, including unresolved effects across restart;
these source observations cannot discharge its runtime tests.

## Production standard self-audit

All 14 rules were checked for this bounded source fixture. No unresolved finding
remains within it.

| Rules | Evidence and limits |
|---|---|
| 1–4: understand, limit scope, maintainability, boundaries | Uses the pinned source observations and existing stdlib SDK probe pattern. Fixed case inventory, explicit executable identity and safe shell/YAML quoting preserve reproducibility. |
| 5–8: failures, secrets, persistence, bounds | Isolated credential-free environment, bounded protocol input/output and deadlines, owned process cleanup, fresh output directories and hashed retained evidence. Missing raw command logs cannot be replaced by summaries. |
| 9–11: tracking, verification, honest reporting | Only the source-probe subtask is complete. Seven actual source cases, 48 corruption controls, independent refusals, artifact hashes, Ruff and Black passed. No host, restart or live-provider claim is made. |
| 12–14: collaboration, self-audit, clear writing | Independent specification and quality reviews closed the concrete false-pass findings. This record states the source/host async difference and remaining limits explicitly. |

## Shared-helper async extension

The optional observer scenario added for [async source qualification](plugin-async-source.md)
retains the ordinary one-shot path. All seven original cases and 48 corruption
controls passed with helper hash
`2fcb097b0d077d512fabe42321fdde99e3400cd95078dfa82fb5405d87c2f682`.
Independent specification and quality reviews replayed this regression and
verified its 69 artifact hashes. The original qualification above remains its
historical evidence; the linked review records the updated helper and run.
