# First coding session implementation

The terminal submission and four-tool cycle use all four production
transport paths with controlled peers. Each cycle reads an unpredictable
seed from an actual file, creates a Python file, edits its value, and runs
a Python assertion through isolated Bash. Live authentication and the
remaining session controls still await their required checks.

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
| scripts/check-coding-session.sh | Build and report only requirements actually checked. |

The first check submits an unpredictable prompt through the real editor,
observes it at each transport peer, and waits for its response in terminal
output. A selected Codex peer that refuses the prompt is the safe violating
case. The same test with a functioning peer is the corrected case. These
fixtures do not prove current subscription service compatibility.

After each action is committed and checked, Cairn selects the next one.
Later CODE
checks exercise streaming while blocked, steering between tools, child
cancellation, continued context, confinement, and retained tool results.
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
