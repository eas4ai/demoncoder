# Claude submit and Stop source observation

Status: bounded source qualification approved by independent specification and quality reviews.

The pinned Claude Code 2.1.267 executable exposes genuine SDK UserPromptSubmit
and Stop callbacks before any tool is used. These observations supply source
facts for the host lifecycle implementation; they do not establish host delivery.

Executable SHA256:
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`.

`tests/plugin_non_tool_source_inputs.py` uses the existing local model/SDK peer
in `tests/plugin_post_source_inputs.py`. It runs with isolated home and explicit
synthetic credentials, bounds stdout and each model request at 2 MiB, and kills/reaps the
source process group on exit. Its fresh output directory retains raw protocol
events, model requests, their interleaved trace, command/environment inputs,
stderr, copied input files and SHA256 references. Python optimization, a wrong
binary pin and an existing output directory are refused.

Observed cases:

| Case | Actual model requests | Source behavior |
|---|---:|---|
| Pass | 1 | Submit callback and reply precede the model request; Stop follows the response. |
| Submit denial | 0 | The result retains the refusal and original prompt but reports source transport success. Host completion must inspect its own gate receipt. |
| Stop correction | 2 | One Stop objection leads to another model request and Stop callback. `stop_hook_active` changes from false to true; no second UserPromptSubmit occurs. |

Both events carry actual source session, transcript, workspace and prompt
identity. The SDK envelope also calls its callback correlation `tool_use_id`;
that name does not establish a tool operation. The event input has no tool
candidate or result. Source Stop correction still needs host authorization
against the original allocation and correction limit before continuation.

The retained run is
`/home/shawn/demoncoder-check-tmp/claude-non-tool-source-3/qualification.json`:
three cases and 31 corrupted-evidence rejections passed. Attacks change both
copies of backend messages where necessary, so trace equality alone cannot
reject forged session, event, prompt, workspace or Stop state. Separate attacks
change replies and move the model request before the submit callback.

This is controlled pinned-source evidence. It does not qualify DemonCoder's
dispatcher, correction accounting, cancellation, command hooks, all source
fields, Codex behavior or live-provider acceptance. The source success result
on denial must never substitute for a satisfied host gate.

The shared helper's original 25 post-tool source cases passed at
`/tmp/demoncoder-post-source-ztzqio9g/result.json` with 100 retained artifacts.
Specification review found two verifier gaps: correction text could be hidden
in unused JSON metadata, and assistant responses could be removed from both
trace copies. Both attacks now fail. The verifier requires actual user text
feedback and a source assistant response between each model request and Stop.
Independent replay verified all 21 source artifacts, three inputs and 31 attacks.
Quality review independently rehashed all 21 source artifacts and three inputs,
replayed 31 attacks, rejected two additional response-ordering attacks, and
rehashed/replayed all 25 shared regression cases. Both reviewers approved the
exact driver SHA256 `9b1fd94c9351c991518e3f54f8636ee6a3de3a67ccd1c7b91ed42d167f3e57cd`
and helper SHA256 `e25931585069d07eab5f69ed150e3bf80848e088712c2fc2a657c9fca10e2e6a`.
They did not rerun the SDK executable.

Operational limits: stderr is captured to disk without a byte cap; HTTP reads
and stdin writes are not hostile-peer containment. This harness runs a trusted,
pinned local source executable. These limits do not relax production runner
confinement or cancellation requirements.

The final source driver passed Black; both Python files passed Ruff. Actual
negative CLI checks rejected optimized Python, a wrong binary and reused output.
Final self-audit found this bounded change consistent with the production rules:
existing transport reuse, isolated synthetic credentials, stable copied inputs,
raw reproducible evidence, meaningful violating cases, reviewed corrections and
explicit limits. Remaining host lifecycle work stays active in the plan.
