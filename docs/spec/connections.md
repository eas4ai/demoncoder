# Provider connections

Status: Agreed 2026-09-06
Prefix: CONN
Host paths: ~/.demoncoder/

The developer requested an expandable provider list and named the following
initial connections. API keys and subscription authentication are distinct
choices, even when the models come from the same company.

| Connection | Proposed adapter | Loop owner |
|---|---|---|
| OpenAI API key | Native OpenAI API adapter | DemonCoder |
| OpenAI subscription | Codex app-server adapter using Codex-managed authentication | Codex |
| Anthropic API key | Native Anthropic API adapter | DemonCoder |
| Claude subscription | Claude Agent SDK/headless CLI adapter using the developer's Claude Code login | Claude Code |

These are proposed integration choices, not claims that the implementations
exist or that their controls are equivalent. The Rust host can launch
`claude -p` directly; using a Python or TypeScript SDK bridge is justified
only by a capability needed for the session contract.

[CONN-001] The application MUST offer each of the four initial connections as a usable coding-session choice.
Falsifier: An initial connection is absent, is only a label or stub, or cannot complete the first-session tool cycle with a valid account through its real transport.
Mechanism: connections; run adapter protocol cases and a recorded live smoke session for every initial connection against a temporary repository.

[CONN-002] The connection registry MUST admit an additional adapter without changes to the coding loop or terminal rendering logic.
Falsifier: Registering a fixture provider and selecting it through normal configuration requires edits to either the loop's provider-specific branches or the terminal renderer.
Mechanism: connections; register an independent fixture adapter through the public interface and run the normal session-creation and event-rendering paths.

The initial plugin shape is a versioned Rust interface plus registration
and configuration. The application composition can register built-in
adapters. A compatible endpoint can be configured through an existing
adapter where its capabilities match. Separately installed executable
adapters can be introduced through a later commitment; a dynamic-library
loader, marketplace, or scripting engine is not initial scope.

[CONN-003] The connection adapter MUST use the authentication method selected by the developer. API keys MUST come from the environment or private connection settings under ~/.demoncoder/, with explicit environment keys taking precedence. Subscription choices MUST use their backend login.
Falsifier: A subscription selection silently uses an ambient API key, an API selection borrows subscription credentials, an authentication failure silently switches accounts or billing methods, or the selected environment/home credential source is ignored.
Mechanism: connections; supply distinct synthetic credentials for both methods, inspect the selected protocol route, and exercise expired and missing authentication.

[CONN-004] The session runtime MUST delegate model/tool progression to exactly one loop owner per session.
Falsifier: Both DemonCoder and an external backend advance the same turn or execute the same requested tool, or resuming a session attaches to an unrelated backend session.
Mechanism: connections; drive recorded backend events through the production adapter and assert one execution, stable backend-session identity, and cancellation of the correct session.

[CONN-005] The application MUST reject a requested capability that the selected connection cannot provide. Saved connection model and effort assignments MUST reach the selected adapter, with explicit CLI selections taking precedence.
Falsifier: The interface reports a requested model feature, steering operation, approval boundary, or cancellation control as active when the adapter ignores or cannot enforce it, or a saved or explicitly overridden model/effort setting is ignored.
Mechanism: connections; select an adapter with explicitly missing capabilities and observe a specific error before the unsupported operation executes.

All initial connections still need to satisfy CODE-001 through CODE-008.
A capability error is honest behavior; it does not turn an incomplete
required connection into completed support. Broader adapters can publish
their supported capability set without claiming universal parity.

[CONN-006] The terminal interface MUST distinguish reported usage from unavailable usage information.
Falsifier: An unreported token dimension or unknown price appears as a measured zero, or usage from one connection is attributed to another.
Mechanism: connections; stream known, partial, and absent usage records through each adapter and inspect the terminal and retained session events.

## Authentication evidence checked during specification

The installed `codex exec --help` and `claude --help` were inspected without
starting a model request. Official [Codex app-server documentation](https://learn.chatgpt.com/docs/app-server#auth-endpoints)
describes managed ChatGPT authentication and session integration. Official
[Claude Agent SDK documentation](https://code.claude.com/docs/en/agent-sdk/overview)
describes the SDK as the Claude Code loop and documents CLI subprocess use
from other languages.

The [Claude subscription usage notice](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan)
currently says the announced usage changes are paused and SDK/headless usage
continues to draw on subscription limits. The SDK overview separately
distinguishes third-party login offerings from its API-key setup. This
proposal uses the developer's local authenticated installation; it does not
promise a public third-party login service or fixed subscription limits.
No credential was read and no live provider call was made for this draft.
