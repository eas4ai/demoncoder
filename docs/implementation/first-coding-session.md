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
Remaining CODE checks exercise confinement and retained tool results.
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

The CODE-002 protocol checks do not establish that external built-in tool
restrictions are complete. CODE-007 still needs adversarial per-connection
admission checks, including the actual installed backend path. No live
provider call was used to build this step.
