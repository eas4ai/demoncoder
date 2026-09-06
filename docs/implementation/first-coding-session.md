# First coding session implementation

The terminal submission and four-tool cycle use all four production
transport paths with controlled peers. Each cycle reads an unpredictable
seed from an actual file, creates a Python file, edits its value, and runs
a Python assertion through isolated Bash. Live authentication is checked
separately with retained two-turn records tied to committed inputs.

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

The ownership check requires one owner and one start/result pair for each
completed operation. Subscription continuation must use the original
backend identity, including after cancellation closes its process. A
changed resume identity is rejected before a queued tool can run; the
preceding workspace remains intact.

| Path | Responsibility |
|---|---|
| src/main.rs | Construct the selected session and own application shutdown. |
| src/config.rs | Load trusted home or explicit connection settings. |
| src/startup.rs | Guide first-start settings, credentials, and project trust. |
| src/oracle.rs | Judge outside-access proposals in a separate session with no tools. |
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

The registration check composes a separately implemented fixture provider
with the public Registry, normal named configuration selection, unchanged
native loop, and production terminal. It reads a real fixture file and
checks its response and events. Adding an adapter needs a factory and
registration in the application composition; it needs no provider branch
in the coding loop or renderer. Runtime plugin loading remains pending.

`register_with_capabilities` declares support for read, write, edit, Bash,
steering, and cancellation. Selection rejects a missing required control
before the factory can start a session. The original `register` method
retains the complete version-1 session contract. Model and effort support
remains adapter-specific; a provider rejection fails without switching
models or retrying automatically.

The usage footer shows the latest reported usage for the active turn. Each
new turn starts with `usage unknown`; it cannot inherit an earlier turn's
counts. Missing input, output, cached-token, and cost fields remain unknown.
An explicitly reported zero stays zero. Claude's reported dollar cost is
shown when present; the other adapters do not invent a price. Retained usage
events carry the selected named connection. `tests/usage.py` streams known,
partial, zero, and absent records through every production adapter and
checks both the current terminal screen and retained events.

The native HTTP paths require an API key from OPENAI_API_KEY or
ANTHROPIC_API_KEY, or from private saved connection settings, and a model. The subscription subprocesses do not inherit either API
key. Codex registers dynamic tools on thread/start. Claude registers an
SDK MCP server and routes MCP messages over its existing control channel;
no Python or TypeScript runtime is added to the application. Each external
backend owns model progression and awaits the shared executor's result.

In the confined default, file tools require relative paths, Linux openat2 without symlinks, regular
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

In the confined default, Codex advertises workspace-write permission to match the admitted host
tools. Its environment selection remains explicitly empty, so its built-in
filesystem tools remain unavailable. The shared executor owns actual
workspace access. The installed-backend confinement checks are rerun when
this policy changes. A live run exposed why this distinction matters: an
advertised read-only policy caused the model to refuse the permitted edits.


## Home configuration

DemonCoder's first interactive start guides connection selection, model,
effort, API credentials or existing subscription login, the default
connection, project trust, and the Oracle assignment. Run
`demoncoder --setup` to reopen this flow. API-key input is hidden before the
prompt becomes visible; cancellation restores terminal settings without
saving. Setup takes an exclusive settings lock and saves through a private
temporary file, synchronization, and atomic replacement. New settings
directories use mode 0700; saved files use 0600.

Project trust uses the canonical selected path and covers its descendants.
An unknown project requires confirmation. `--trust-workspace` authorizes
only this invocation. `--config` can supply setup choices to an automated
invocation, but does not itself grant project trust. Unrecognized plugins
and hooks are not enabled; dynamic extension loading remains pending.

DemonCoder loads `~/.demoncoder/settings.toml` automatically. `--config`
selects a complete alternative file. Repository settings are never loaded
automatically. The older `~/.demoncoder/config.toml` is preserved and is not
interpreted as this format.

```toml
default_connection = "codex"

[oracle]
connection = "codex"
model = "gpt-6-astra"
effort = "medium"

[connections.codex]
adapter = "codex"
model = "gpt-6-astra"
effort = "medium"

[connections.claude]
adapter = "claude"
model = "sonnet"
effort = "medium"

[connections.openai-api]
adapter = "openai-api"
model = "your-openai-model"
# api_key = "your-key" # optional when OPENAI_API_KEY is set

[connections.anthropic-api]
adapter = "anthropic-api"
model = "your-anthropic-model"
# api_key = "your-key" # optional when ANTHROPIC_API_KEY is set
```

Use `chmod 600 ~/.demoncoder/settings.toml` when saving credentials.
Credential files must belong to the current user and exclude all group and
other permissions. Files are bounded to 64 KiB and must be regular files;
symlinks are rejected. Configuration parse errors never quote their input.
This file is plaintext; no custom encryption or keyring integration is claimed.

`--connection`, `--model`, and `--effort` override saved selections.
Nonempty environment API keys override saved keys. An explicitly empty key
reports an error rather than selecting a different credential. Subscription
connections reject saved API keys and do not inherit ambient API credentials.
Use `codex login` or `claude auth login` for those connections. `CODEX_HOME`
and `CLAUDE_CONFIG_DIR` select their existing backend login directories;
DemonCoder leaves HOME intact.

Subscription admission requires the backend to confirm its route. Codex
must report a ChatGPT account before a thread starts. Claude must report
`apiKeySource: none` before model output, successful completion, or a tool
call is accepted. Missing or mismatched route information fails closed.
Closing Claude's backend clears that confirmation; a resumed process must
confirm it again. Missing and expired authentication never trigger an
automatic credential, account, model, or billing fallback.

Effort goes to OpenAI Responses `reasoning.effort`, Anthropic Messages
`output_config.effort`, Codex `turn/start.effort`, or Claude `--effort`.
Omission leaves the backend/provider default. Anthropic effort controls
response effort; it does not itself enable an older model's extended-thinking
mode. Model-specific support remains the provider's responsibility; rejected
settings produce a failed request with no fallback model or effort.
Named connections assign settings to a session. Subagent role assignments
remain in their later commitment.

Reference inspection: T3Code 0.0.38's `docs/user/providers-claude.md` and
`providers-codex.md` document normal CLI logins and separate configuration
directories. Its `ClaudeAdapter.ts` passes the installed executable, model,
effort, and resume identity to the SDK. `CodexSessionRuntime.ts` sends effort
in `turn/start`. Its relay infrastructure handles remote-environment login.
These observations informed the adapter checks; no T3Code source was copied.

## Explicit host execution

`demoncoder --yolo` selects host access for that invocation. The terminal
labels it `HOST ACCESS`. Bash runs on the host in the selected project with
no sandbox or routine permission prompts. It receives a private mode-0700
scratch directory under `/tmp` through `TMPDIR`; provider API keys are
excluded from its environment. Native tools can use absolute paths,
parent paths, and symlinks. The confined default continues to require
bubblewrap and never falls back to host execution.

Host mode requires a configured Oracle. Every Bash request and every native
file request outside the project or owned scratch directory is reviewed
after hooks have transformed its arguments. Hard-linked files also receive
review because another name may lie outside those roots. Existing files
are opened without truncation or creation, and the Oracle sees the resolved
descriptor target. A new file is created exclusively through its pinned
parent only after review. A target or parent moved during review causes a
failure, and a concurrently created leaf is never replaced.

The Oracle receives the developer task, final proposal, project, scratch,
home, and resolved target as separate fields. Its session advertises no
tools and rejects tool requests. A denial, invalid result, provider error,
tool request, or 60-second deadline blocks the operation. Review reasons
and Oracle usage have separate events and terminal labels. Original tool
results remain authoritative. The guard is a model judgment and cannot
prove arbitrary shell effects safe; it is not filesystem isolation.

Host Bash keeps the 120-second and 1-MiB limits. Completion and cancellation
kill its process group before reaping the leader, including ordinary child
processes that hold output pipes open or close their output early. Programs
that deliberately detach into a different session are outside that cleanup
guarantee. Scratch files are not recursively deleted at session close.

`tests/onboarding.py` drives setup, saved selections, denied trust, hidden
credentials, and cancellation through a real pseudo-terminal.
`tests/host_access.py` exercises all coding connections and native Oracle
transports; `tests/host_guard.rs` covers subscription Oracle transports,
hook arguments, scratch, cancellation, timeout, target swaps, and host
children. `tests/live_oracle.py --run` records one live allow/deny pair
through the production Oracle API. It submits a harmless outside read and
a home-move proposal for judgment only; neither proposed tool is executed.
The ordinary mechanism validates the retained record against committed
inputs and makes no extra live call.
