# First coding session implementation

The terminal submission and four-tool cycle use all four production
transport paths with controlled peers. Each cycle reads an unpredictable
seed from an actual file, creates a Python file, edits its value, and runs
a Python assertion through isolated Bash. Live authentication and the
remaining session controls still await their required checks.

The responsiveness driver holds each provider and then a real Bash command
open. It observes incremental output and typed editor text before releasing
either operation for all four adapters.

Enter submits a correction while the session is working. The active tool
finishes and retains its result; superseded queued tools are denied. Native
providers receive the correction before their next request. External
backends receive the completed result, then an interruption request. After
both interruption acknowledgement and turn completion, the adapter submits
the correction in the same backend thread or session. A backend that does
not complete this transition within 30 seconds reports an error. This is
the steering transition limit; cancellation has its separate CODE-005 check.

Escape cancels the active turn with a two-second grace period. Native HTTP
requests are dropped and isolated Bash processes are killed with their PID
namespace. External backend processes run in a separate process group;
cleanup kills that group before reaping its leader, including on owner drop.
The terminal remains open and accepts another prompt. Codex reconnects with
thread/resume and Claude with --resume using the preceding backend identity.
Native sessions keep each completed tool result as it arrives and close
unfinished tool calls with an explicit unknown-result notice. The next
prompt keeps the preceding conversation and completed workspace changes.
Closing a client request cannot establish the provider's billing outcome;
unreported cost remains unknown.

| Path | Responsibility |
|---|---|
| src/main.rs | Construct the selected session and own application shutdown. |
| src/config.rs | Parse explicit trusted connection configuration. |
| src/session.rs | Versioned adapter registry and session lifecycle. |
| src/native.rs | Shared direct-provider model/tool loop. |
| src/tools.rs | Typed requests, final admission, rooted files, isolated Bash, and actual results. |
| src/events.rs | Ordered session events, optional retained output, and UI delivery. |
| src/terminal.rs | Responsive terminal rendering and prompt editing. |
| src/adapters/ | HTTP and backend protocol boundaries. |
| tests/terminal_session.py | Drive the actual binary through a pseudo-terminal. |
| tests/backend_fixture.py | Controlled Codex and Claude protocol peers. |
| tests/responsiveness.py | Observe output and editor input before provider and tool release. |
| tests/steering.py | Submit a correction during a tool and inspect the next model input and actual effects. |
| tests/steering_fixture.py | Queue superseded and late backend tool requests around interruption. |
| tests/cancellation.py | Observe HTTP closure, terminated children, stopped file activity, and a new prompt after Escape. |
| tests/continuation.py | Extend a uniquely named function in a second turn after completion or cancellation. |
| tests/continuation_fixture.py | Keep backend-owned context and require resumption of its original identity. |
| scripts/check-coding-session.sh | Build and report only requirements actually checked. |

The first check submits an unpredictable prompt through the real editor,
observes it at each transport peer, and waits for its response in terminal
output. A selected Codex peer that refuses the prompt is the safe violating
case. The same test with a functioning peer is the corrected case. These
fixtures do not prove current subscription service compatibility.

After each action is committed and checked, Cairn selects the next one.
The CODE checks cover the first session's controlled runtime cases.
CONN checks include independent registration and live two-turn tasks for
all four connections. Missing authentication remains unresolved evidence.

The native HTTP paths require OPENAI_API_KEY or ANTHROPIC_API_KEY and an
explicit model. The subscription subprocesses do not inherit either API
key. Codex registers dynamic tools on thread/start. Claude registers an
SDK MCP server and routes MCP messages over its existing control channel;
no Python or TypeScript runtime is added to the application. Each external
backend owns model progression and awaits the shared executor's result.

File tools require relative paths, Linux openat2 without symlinks, regular
UTF-8 files without hard links, and at most 1 MiB. Parent directories must
already exist. Bash requires /usr/bin/bwrap, isolated namespaces, a clean
environment, read-only system binaries, and the pinned workspace directory.
It accepts workspaces without links or special files, protects .git from
Bash writes, and limits execution to 120 seconds and output to 1 MiB.
These are current restrictions; broader tool usability remains review work.

CODE-007 adds per-connection admission checks with sibling and credential
canaries, protected Git state, and permitted file and Bash operations.
The two subscription cases run installed Codex 0.153.4 and Claude Code
2.1.263 against local model responses and synthetic authentication.
Every model request must advertise exactly the four host tools. Forced
requests for backend built-ins must fail without exposing or changing
the canaries. An inherited Codex MCP server must never start.

Codex starts threads and turns with an explicitly empty environment list,
which removes its built-in filesystem execution path. It reads inherited
MCP configuration and disables each server explicitly; an empty table alone
does not override inherited servers. Other tool sources are disabled.
Inbound JSON-RPC requests and replies have separate ID spaces; a server
tool request with the same numeric ID as a pending client request remains
a request. Claude exposes only the host SDK MCP tool set.

These installed-backend checks exercise the real local runtime and tools.
They do not use live provider authentication or establish live service
compatibility. `tests/installed_backends.py` and `tests/boundary_fixture.py`
retain the cases; `tests/installed_backend_launcher.py` launches the actual
binary with local test endpoints. The separate tools tests verify denied
hooks, final transformed paths and commands, and allowed transformations.

CODE-008 runs a failing Python repository assertion, applies a correction,
and reruns the same assertion successfully through all four connections.
The installed-backend driver waits for the failed check to appear in the
terminal before it supplies the correction. It compares the original event
receipts with the model's actual tool results, including identity, output,
success, and exit code. A presentation hook has a separate labeled event;
it cannot replace the original result.

The executor retains one completed receipt before awaiting event delivery
or invoking presentation hooks. If cancellation or a presentation error
interrupts that delivery, the native session supplies the known result to
its model before closing any remaining calls as uncertain. The receipt is
cleared after delivery and cannot be delivered twice. This handles in-process
interruption; durable crash recovery remains later scope.

The connection smoke driver is `tests/live_connections.py`. After committing
and building the current inputs, run it with `--run <connection>` and an
explicit `--model <model>` for API connections. It uses the default live
endpoint and current selected authentication. It opens a temporary repository
through the real terminal, creates and verifies one simple function, then
extends and verifies it in a second turn. It parses the resulting source
without executing generated code on the host. All executed checks go through
the application's isolated Bash tool.

Redacted records live under `.cairn/evidence/live/`. They contain the two
turns' events and final source, selected connection, backend version, and a
digest of committed source, build, live-driver imports, and contract inputs. The
ordinary connection check validates these records without making additional
provider calls. A missing, unsuccessful, or stale record remains unverified.
Current live results are reported by Cairn; the existence of the driver
does not establish a successful connection.

The OpenAI adapter explicitly requests encrypted reasoning items while
using `store: false`, and retains them with the rest of the response for
subsequent tool decisions and turns. This follows the official
[Responses API contract](https://developers.openai.com/api/reference/cli/resources/responses/methods/create).
The controlled tool-cycle test checks both the requested inclusion and
the returned item's presence in the next request.

Codex advertises workspace-write permission to match the admitted host
tools. Its environment selection remains explicitly empty, so its built-in
filesystem tools remain unavailable. The shared executor owns actual
workspace access. The installed-backend confinement checks are rerun when
this policy changes. A live run exposed why this distinction matters: an
advertised read-only policy caused the model to refuse the permitted edits.
