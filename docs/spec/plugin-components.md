# Plugin components and distribution

Status: Agreed 2026-09-09
Prefix: PLUG

These requirements belong to the same complete deliverable as
[skills and packages](extensions.md) and [lifecycle hooks](lifecycle-hooks.md).
Implementation order does not make any component optional for completion.

[PLUG-001] Plugin agent templates MUST create assignments through the existing
subagent manager, preserving explicit ownership, confined worktrees, cumulative
allocations, visible status and developer-controlled integration. Template model,
tool and skill requirements MUST be validated before assignment.
Falsifier: A template starts a hidden backend agent, broadens a child's tools,
silently substitutes its requested model, or integrates changes without approval.
Mechanism: Assign a supported template and templates with impossible model/tool
requirements; test attempted ownership escape and explicit validated integration.

[PLUG-002] Plugin MCP declarations MUST use managed, explicitly authorized
connections whose tools pass through shared admission, cancellation and evidence
recording. Plugin LSP declarations MUST use the existing managed language-service
boundary and retain its filtered filesystem and freshness rules.
Falsifier: An MCP tool bypasses pre-tool gates, an LSP bundle exposes protected
files, or disabling a plugin leaves its service accepting new requests.
Mechanism: Exercise controlled stdio and Streamable HTTP MCP services and a
fixture LSP bundle, including malicious server requests, cancellation and disable.

[PLUG-003] HTTP and MCP hook handlers MUST declare their destination, payload
access and authentication binding. They MUST obey the same gate deadlines,
decision validation and durable accounting as command handlers. Secret values
MUST NOT be stored in package manifests, prompts or ordinary hook logs.
Falsifier: Redirects send an authorized payload to an unauthorized endpoint,
a failed remote gate permits work, or a credential appears in an exported receipt.
Mechanism: Use local fixture endpoints to test destination changes, timeouts,
invalid replies, explicit credential binding and redacted exported evidence.

[PLUG-004] Plugin monitors MUST run as bounded, visible, owned background
processes with declared triggers and access. Their messages MUST enter a bounded
queue as attributed external observations. Overflow and crashes MUST be reported.
Messages MUST NOT become developer commands or bypass safe tool boundaries.
Falsifier: A monitor spoofs the developer acceptance command, flooding grows memory indefinitely,
a message interrupts a mutation midway, or the process survives its owner.
Mechanism: Flood and crash a harmless monitor, inject command-shaped messages,
and inspect queue bounds, delivery timing, attribution and shutdown.

[PLUG-005] Remote package installation and update MUST resolve reviewed source
identities and immutable content digests before activation. Dependencies MUST be
bounded, cycle-checked and explicitly included in the activation review. Failure
MUST preserve the previously working generation.
Falsifier: A moved tag silently changes a running plugin, an update executes
undeclared dependency installation scripts, or partial download loses the old package.
Mechanism: Use a controlled registry and changing references to test pinning,
dependency cycles, interrupted fetches and update rollback without external publication.

[PLUG-006] The application MUST support plugin configuration forms with typed
defaults, required values and sensitive-value handling. It MUST bind apps and
connectors to explicitly configured service identities and developer-authorized
authentication, and report provider-controlled access restrictions accurately.
Secret values MUST remain outside instruction content, package files and ordinary
logs. Substitution into shell source MUST be rejected.
Falsifier: A repository supplies a secret-bearing destination without authorization,
a required option is ignored, a credential leaks into a skill prompt, or an app
is shown as connected when its provider denied access.
Mechanism: Exercise configuration validation, explicit authentication, revocation,
missing provider entitlement and shell-injection canaries with local services;
include authorized live service smoke evidence for the connector binding.

[PLUG-007] The application MUST discover, select and persist plugin themes and
output styles. Theme token mappings MUST be validated against the terminal's
supported palette. Editing a bundled theme MUST create an editable user copy.
Output styles MUST preserve original evidence and the host's control instructions.
Falsifier: Disabling a theme leaves unreadable terminal colors, editing modifies
the installed package, or an output style hides a failed check in durable state.
Mechanism: Load valid/invalid themes, switch and restart, edit a copy, disable the
source package and test output styles against retained failure evidence.

[PLUG-008] Plugin channels MUST bind to an admitted package MCP server and
authenticate their external source. Inbound messages MUST retain source identity,
obey queue and allocation bounds, and enter the session only at safe boundaries.
External content MUST NOT be treated as developer approval or control commands.
Falsifier: An unauthenticated sender injects a message, a channel for one workspace
reaches another, or a message that requests acceptance accepts work.
Mechanism: Deliver authenticated, forged, duplicate, oversized and out-of-order
fixture messages; inspect attribution, deduplication, routing and control state.

[PLUG-009] The application MUST support local, project, user and managed package
scopes, skills-directory discovery, marketplace catalogs, pinned remote sources,
updates, uninstall, author scaffolding and validation. Scope resolution MUST be deterministic
and visible. Dependency changes MUST be reviewed as part of activation.
Falsifier: A project package silently overrides managed policy, two sources shadow
each other without explanation, removal breaks dependents without warning, or a
marketplace's default-enabled flag executes code without activation authorization.
Mechanism: Exercise all scopes with conflicting identities, required dependencies,
changed versions, source removal and offline operation through the public controls.

[PLUG-010] Plugin workflows and skill execution directives MUST route dynamic
context commands, context forks, tool restrictions and agent selection through
existing admitted operations. Declared executable Markdown blocks MUST remain
distinct from literal invocation arguments and consume visible allocations.
Falsifier: A dynamic context block executes before policy admission, a context fork
creates a hidden unconfined agent, or user argument text becomes executable source.
Mechanism: Run workflow and skill fixtures with dynamic context, explicit forks,
restricted tools and adversarial arguments through all four connections.

[PLUG-011] Managed plugin services MUST implement declared startup, initialization,
restart and shutdown policy with bounded retries and actionable status. Reload
MUST retain an unchanged admitted service when identity, configuration and policy
are unchanged. Conflicting LSP language registrations MUST have a visible,
deterministic selection with fallback after initialization failure.
Falsifier: Reload needlessly loses an unchanged connection, a crashing server
restarts forever, or an invalid LSP registration prevents a valid fallback from starting.
Mechanism: Reload unchanged and changed service declarations, crash fixture
servers, exceed restart limits and fail the preferred language registration;
inspect connection identities, process ownership and selected fallback.
