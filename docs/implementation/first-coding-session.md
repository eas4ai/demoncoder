# First coding session implementation

The current action is CODE-001: submit a prompt in the terminal and observe
the selected runtime begin it. The initial change uses the four production
transport paths with controlled peers. It does not claim tool support,
safe steering, recovery, or live authentication evidence yet.

| Path | Responsibility |
|---|---|
| src/main.rs | Construct the selected session and own application shutdown. |
| src/config.rs | Parse explicit trusted connection configuration. |
| src/session.rs | Versioned adapter registry and session lifecycle. |
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

After this action is committed and checked, Cairn selects the next one.
CODE-002 adds the four tools through the production executor. Later CODE
checks exercise streaming while blocked, steering between tools, child
cancellation, continued context, confinement, and retained tool results.
CONN checks include independent registration and live two-turn tasks for
all four connections. Missing authentication remains unresolved evidence.

The native HTTP paths currently require OPENAI_API_KEY or ANTHROPIC_API_KEY
and an explicit model. The subscription subprocesses do not inherit either
API key. Native tools are not yet registered. Backend tool configuration
and confinement remain unverified until the shared tool executor and its
checks are built. No live provider call was used to build this step.
