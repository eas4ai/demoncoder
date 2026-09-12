# Codex submit and Stop source observation

Status: bounded source qualification approved by independent specification and quality reviews.

The pinned managed Codex 0.153.4 executable runs configured command hooks at
UserPromptSubmit and Stop without a tool call. Source-generated inputs contain
the actual thread, turn, transcript, workspace, model and permission mode.
The host can use these genuine boundaries; creating a future backend turn ID
before its observed submission would not establish the same facts.

Executable SHA256:
`80315a32acf1b625129a46b0bd75537cf76ef09701ae986154159076cc0b6aff`.

`tests/plugin_codex_non_tool_source.py` extends the existing synthetic HTTPS
and authenticated app-server peer through a keyword-only lifecycle option.
The default post-tool and async cases retain their original behavior.
Only the private fixture's exact hook command hashes are trusted. The child
receives explicit synthetic authentication, isolated home and loopback proxy
configuration. No ambient credentials are copied into that environment.

The retained run at
`/home/shawn/demoncoder-check-tmp/codex-non-tool-source-2/qualification.json`
passed three cases and 30 corrupted-evidence controls:

| Case | Actual model requests | Source observations |
|---|---:|---|
| Pass | 1 | Submit and Stop command inputs have no fabricated tool candidate. |
| Submit denial | 0 | The hook is blocked, while the source turn still reports completed. Host gate state remains independently authoritative. |
| Stop correction | 2 | One original submission, Stop states false then true, and corrective user text in the next model request. |

The verifier compares command captures with timing records, source hook
started/completed events, actual assistant responses and terminal turn identity.
It verifies that submit precedes model work, each assistant response precedes
its Stop event, and the Stop correction reaches actual user input text.
Corruption controls change session/turn/event/prompt/workspace, model counts,
Stop state and ordering, remove assistant evidence, and hide correction text
in unused metadata. Source transport completion never proves a host gate passed.

Fresh output directories retain copied inputs, command/environment configuration,
source command code, exact trusted configuration, stdout event traces, model
requests, command captures, timing, stderr and artifact hashes. Input hashes and
the executable pin are checked before and after execution. Optimized Python,
wrong executables and reused output directories were each tested and refused.
Black passed for the new driver; Ruff passed for it and the shared helper.

Limits: controlled pinned-source evidence, not DemonCoder lifecycle dispatch,
authenticated host relay qualification, host allowance/correction accounting,
cancellation, MCP handlers or live-provider acceptance. Stdout and individual
model request bodies are bounded; stderr is captured without a byte cap. This
local trusted-source harness is not hostile-process containment. The existing
TLS fixture also produces expected websocket fallback diagnostics; the retained
requests establish the actual completed local HTTP model exchange.

Shared regression runs passed: three post-tool cases with 12 artifacts at
`/tmp/demoncoder-codex-post-source-wkssfml5/result.json`, and three async cases
with 38 corruption checks at
`/home/shawn/demoncoder-check-tmp/codex-non-tool-async-regression-1/qualification.json`.

Specification review found that coordinated changes could forge transcript
provenance, detach the correction hook identity, or move Stop completion past
the correction request. The verifier now binds these to the raw thread/turn
acknowledgments, the actual completed hook ID and model-request ordering.
Fresh source cases and all three original attacks passed re-review: 35 artifact
hashes, five inputs, three cases and 30 corruption checks were independently
verified. Quality review independently verified those same hashes and replays,
rejected six more coordinated permission/status attacks, and rehashed the 42
shared-regression artifacts. Neither reviewer reran the source executable.

Approved driver SHA256:
`f9a429bef73f346fc088a055368883c03be903bfeff30c8c24ecdaa2b87a8aa8`.
Approved shared helper SHA256:
`3c52a4410b88821b449a7883fafc5c28d76ce2779bb0a2b5586321c8e37a79ea`.
Frozen files form a focused evidence bundle; replay imports also use repository
test dependencies. They are not a standalone Python distribution.

Final self-audit: the source-only change reuses the existing peer, preserves
default post/async behavior, retains raw identity and causal evidence, isolates
synthetic credentials, and includes meaningful failure controls and independent
review. Known harness limits are explicit. Production adapter dispatch remains
unimplemented in this record and is still required by the active plan.
