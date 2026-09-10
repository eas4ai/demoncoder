# Plugin compatibility contract

Status: Draft 2026-09-09
Prefix: PCOMP

This is part of the complete plugin deliverable. The versioned
[profile inventory](compatibility/plugin-profile-v1.json) fixes the source
revisions, field inventory, event set and real package fixtures. A reference
profile defines reproducible compatibility, not permission to drop a feature.

[PCOMP-001] Importers MUST implement the manifest precedence and path composition
below. A supported overlay MUST NOT be treated as an alternative package.
Falsifier: Root skills or MCP declarations disappear when a Codex overlay adds
hooks, or an inline extension causes the overlay's handlers to execute twice.
Mechanism: Load root-only, overlay-only, combined, inline-replacement and conflicting
fixtures; inspect exact component identities and real execution counts.

[PCOMP-002] Importers MUST implement every event and field in profile v1 using
the wire, dispatch and effect rules below. Documented special results MUST NOT
be rejected merely because they are not generic decision JSON.
Falsifier: A valid WorktreeCreate path is rejected, FileChanged cannot update its
watch list, Codex PostCompact cannot stop continuation, or concurrent handlers deadlock.
Mechanism: Cover every inventoried field and valid event/type pair with positive
and negative production cases, including the special results and concurrency traces.

[PCOMP-003] Each connection MUST implement the event ownership and synchronous
bridge protocol below. The backend MUST NOT cross a guarded boundary without a
valid host acknowledgment. An adapter observation after the fact MUST NOT count
as a successful pre-action veto.
Falsifier: Actual automatic backend compaction proceeds after denial or bridge
failure, a forged callback is accepted, or an explicit host batch is mistaken for
an unobserved backend batch.
Mechanism: Trigger real manual/automatic compaction and host batch/workspace changes
on each qualified backend; deny, allow, disconnect, time out and crash each bridge boundary.

[PCOMP-004] Completion MUST cover the entire frozen inventory and pinned real
fixtures. The application MUST implement the explicit registered-app binding
flow below. A package chosen only because it is easy to pass MUST NOT replace an
inventoried feature or fixture.
Falsifier: Removing a field handler leaves coverage green, a real required fixture
is silently replaced, or a configured Supabase app points at a different service.
Mechanism: Audit generated coverage against the frozen inventory, disable one
nontrivial handler to prove failure, restore it, and exercise the pinned packages
and authenticated read-only connector smoke case through the installed application.

## Manifest precedence

| Package shape | Identity and components | OpenAI-specific settings |
|---|---|---|
| Recognized portable root manifest | Root identity; fixed `skills/` and `mcp.json` components | Inline `extensions.com.openai` object if present; otherwise `.codex-plugin/plugin.json` overlay |
| Portable root plus both inline object and overlay | Root remains canonical | Inline object replaces the overlay as a whole; no merging and no duplicate handlers |
| Legacy Codex only | `.codex-plugin/plugin.json`, with its documented default/custom paths | Same manifest |
| Claude only, or manifest-free Claude directory | Claude manifest/discovery rules | Foreign display metadata retained without being executable |
| Independent Claude and Codex legacy manifests without a portable root | Developer selects the intended import profile; identities and exclusions shown | No accidental union of two independent runtime declarations |

A portable root plus Claude metadata is not permission to import the same skills
twice. Fields in a Codex overlay cannot override portable identity or add/replace
portable skill/MCP roots. A wrong-typed inline extension is an error, not absence.

For Claude, custom `commands`, `agents`, `workflows`, `outputStyles`, themes and
monitors replace their defaults. Custom skill paths add to the default skills
directory, except the documented marketplace-root source rule, which replaces
that scan. Root-skill fallback applies only without a skill directory/declaration.
Normalize and deduplicate identical canonical files before constructing component
identities. Distinct files with the same qualified identity are an error.
MCP/LSP default-file and inline sources combine by server name; conflicting
definitions require an explicit selection rather than order-dependent overwriting.
Hooks retain distinct package/skill handler identities; duplicate references to
the same canonical declaration are loaded once. The activation report records
every replacement, addition, conflict and deliberate host-policy difference.

## Profile and wire rules

Profile v1 includes all 33 Claude SDK event names plus native/Codex Interrupt.
It includes Elicitation and ElicitationResult; the earlier overview omitted them.
The profile records complete input/output type graphs and immutable
upstream source locations. For Codex, the retained full schemas describe accepted
shape, while event behavior below determines whether a parsed field has an effect.
A parsed but ignored upstream field must remain identified as such; parsing alone
does not promote it into a permission or state change.

All handlers run in DemonCoder. Claude command/http/mcp_tool/prompt/agent handlers
are supported. Codex command/mcp_tool declarations use the Codex profile; prompt
and agent tags accepted but not executed by that upstream release are not falsely
described as existing Codex behavior. The developer may explicitly choose the
native prompt/agent runner for them. Native declarations support all five types.
That explicit semantic selection is recorded with the package; the functionality
is delivered rather than deferred.

### Wire graph and coverage identity

Profile v1 revision 2 replaces the flattened Claude declaration list with a
TypeScript-parser-derived graph. Object properties remain nested; intersections
retain inherited references; unions retain each alternative with a content-based
branch ID. A required field is required inside its containing object, not at the
root of every event. `field_paths` are declaration-site JSON pointers. Coverage
must follow references and every branch from each recorded event/control root;
counting field names is insufficient.

The graph includes only the 77 types reachable from its recorded hook/control
roots. General SDK Settings and session-store APIs accidentally captured by the
earlier extractor are not plugin execution requirements. Their removal does not
remove any plugin component, configuration field or feature family. Callback
wrappers and the four named external types have explicit meanings in the graph;
they are not fictitious JSON fields.

Full Codex and portable JSON schemas preserve their original constraints and
reference structure. A selected Codex configuration definition includes its
reference closure. Legacy flat field lists are discovery indices only; they do
not supersede these schemas. Structural keywords such as `oneOf`, `allOf` and
required fields must remain effective in runtime validation.

The [inventory checks](compatibility/README.md) regenerate these records from
hash-verified sources. Positive and negative TypeScript probes compare the pinned
SDK with types reconstructed from the graph. Deleting a nested field or a union
branch must break those probes. These checks establish inventory fidelity only;
the implementation still needs production-path parsing, effect and deletion tests.

### Exact event and handler applicability

`hook_applicability` in the inventory is the complete finite lookup for all
34 events, five handler types and three dialects: 510 cells. Resolve an event's
explicit group, then the handler's explicit status. No default or inference from
a neighboring event is permitted. A missing cell fails inventory validation.

Claude's model-capable events use all five runners; its service-only group uses
command, HTTP and MCP; SessionStart and Setup use command and MCP only. Codex's
12 recorded events use command/MCP; its model tags are nonexecuting source forms.
Native declarations support every listed type, with effects constrained by the
event. The inventory enumerates membership, including events absent from each
source profile. It also specifies the activation and negative tests for every cell.

An unsupported source pair cannot silently execute under another dialect.
**Convert to native** creates a separately validated native declaration after
the developer inspects its changed behavior and supplies required runner settings.
It is not an import fallback or a source-compatibility pass. Source rejection and
working native functionality are both required evidence where that cell names
conversion. A source handler whose result has no decision effect cannot be
registered as a required policy gate; that requires an explicit native declaration.

| Handler type | Input delivery | Result decoding |
|---|---|---|
| command | One bounded event JSON on stdin; declared argv or shell, environment and cwd | Exit status plus dialect/event stdout parser; stderr retained with secret-safe bounds |
| http | Same JSON in POST body; allowlisted headers and credential binding | Successful bounded body decoded for that event; transport/non-success/malformed gate responses hold admission |
| mcp_tool | Resolve declared input placeholders as data; call an already admitted managed server/tool | Structured or text result decoded under the event profile; `isError` is a handler failure |
| prompt | Literal event expansion into a bounded tool-free request | Validated Boolean/verdict response becomes an event-appropriate decision |
| agent | Snapshot-bound isolated read-only assignment | Validated final verdict after retained inspection evidence; no candidate mutation |

### Model responses and task outcomes

`model_response_schemas` defines model responses separately from SDK callback
JSON. `ok` is required, and a false result requires `reason`. Only prompt responses
admit `impossible`. `continueOnBlock` is a prompt configuration field, not an agent
field or a response field. Invalid output leaves a required gate held; an invalid
observer result remains visible without reversing the observed operation.

The frozen `model_result_rules` table gives both prompt and agent outcomes for
every applicable Claude event, plus every native event. It includes source
results that are intentionally ignored. The
[source response rules](https://code.claude.com/docs/en/hooks#response-schema)
establish these distinctions: PermissionRequest/PermissionDenied model results
have no decision effect; Stop/SubagentStop prompt `impossible` can end a turn;
PreToolUse/PostToolUse and teammate transitions distinguish prompt continuation
settings from agent behavior. The explicit inventory lookup, not a generic Boolean
conversion, governs dispatch. TaskCompleted's owner records whether the boundary
is a task-tool transition or a teammate stopping; payload text cannot choose it.

In every dialect, ending a turn or stopping a teammate is distinct from satisfying
a task's completion policy. An `impossible` result stops without another correction
but leaves the required gate unmet. A false result never becomes approval merely
because correction is impossible. A true result removes only that handler's
objection for the exact candidate; ordinary access, other gates, verification,
review and developer acceptance remain necessary.

Requested follow-up uses the existing cumulative allocation. Cancellation,
expired time or exhausted corrections stops work with unmet gates; no source
continuation setting creates a new allowance. Native observation-only events
cannot carry required gates. Native prompt/agent WorktreeCreate handlers may
judge creation, but a Boolean result supplies no path: the admitted host operation
or typed creator must still create and validate the worktree. The equivalent
rule applies to special elicitation and display results.

Hook server startup is a separate admitted dependency operation before dispatch,
not a recursive hook call that waits on itself. Detect dependency cycles such as
an initialization gate requiring the same uninitialized server. Show the exact
cycle and hold activation until an independent bootstrap or configuration repair
resolves it. No automatic OAuth dialogue starts inside a gate.

Zero exit permits decoding; it is not automatically a pass. Exit 2 applies the
event's blocking/continuation rule. Other failures in a required gate hold work,
even where upstream defaults would continue. Observers retain errors without
undoing the observation. These stricter host failure and authority rules are
explicit activation differences. Unknown JSON fields are diagnosed; recognized
fields whose source contract ignores them do not gain an effect.

Common fields include identity, path context, role and event-specific payload
from the inventory. Adapter provenance is retained separately. Optional unknown
usage/cache/backend values remain absent, never fabricated as zero. An upstream
required field with no truthful host equivalent makes that particular response
unrepresentable; the implementation must supply the real source or resolve the
specific contract conflict before completion. A compatibility warning cannot
discharge PCOMP-004.

## Event effects and matching

The table composes with every valid handler type above and the inventory's exact
wire fields. `G` gates the pending action, `C` can stop/request bounded continuation
after an operation, `O` observes without reversing it, and `S` has a special result.
Native events not named by a source dialect use native schemas; importers do not
pretend an upstream runtime emits them. Model/tool output always retains its original
receipt separately from context or display changes.

| Event | Matcher input | Native effect and source mapping |
|---|---|---|
| SessionStart | startup/resume/clear/compact/fork source | O/S: bounded context, title, attributed initial message, watch paths and staged skill rescan; Codex `continue:false` holds subsequent work |
| UserPromptSubmit | none | G/S: submitted text and origin; block or add context/title; machine prompts never acquire developer origin |
| UserPromptExpansion | command name/type | G/S: explicit skill/MCP prompt expansion; suppress-original only changes the block display |
| InstructionsLoaded | instruction memory type/source | O: actual file and load reason; no additional instruction authority |
| PreToolUse | canonical tool and documented aliases | G: final-candidate protocol, deny/ask/defer/updatedInput; `allow` cannot exceed host policy |
| PermissionRequest | tool name | G: answer or proposed permission change goes through developer/managed authority; never a plugin-granted bypass |
| PermissionDenied | tool name | O/S: bounded retry request only for a retryable denial; new admission and same allowance required |
| PostToolUse | tool name | C/S: context and supported model-facing output replacement, never replacement of the actual receipt; classifier assertions from plugins remain untrusted context |
| PostToolUseFailure | tool name | C/O according to dialect: preserve actual failure and interruption state; attach context without pretending success |
| PostToolBatch | none | O/S: one completed declared batch with each tool result; bounded batch context |
| Stop | none | C: bounded correction for task work; `stop_hook_active` prevents fresh correction allowance |
| StopFailure | error category | O: failed termination remains failed; ignored upstream decisions do not cause continuation |
| Interrupt | none | O: cancellation cannot be vetoed; teardown remains bounded |
| SessionEnd | termination reason | O: never veto shutdown; kill unfinished handlers with the owner |
| TaskCreated / TaskCompleted | none | G: proposed task transition and identity; completed work remains distinct from developer acceptance |
| SubagentStart | agent type | O/S: actual child identity and bounded child context; no hidden extra agent |
| SubagentStop | agent type | C: bounded child follow-up before stopped state; cannot prevent cancellation |
| TeammateIdle | none | C: before real idle transition; follow-up consumes the same assignment allowance |
| WorktreeCreate | none | S: command's last nonempty stdout line is a path; JSON HTTP/MCP/callback result uses `worktreePath`; nonzero command exit fails creation |
| WorktreeRemove | none | O: actual owned worktree cleanup; no authority to delete an arbitrary supplied path |
| PreCompact | manual/auto | G: Claude block/exit 2 and Codex `continue:false` deny compaction; preserve the source's recovery-from-context-limit failure behavior |
| PostCompact | manual/auto | O/C: retain actual compacted state; Codex `continue:false` holds continuation after compaction, never undoes it |
| PreModelSwitch | requested model/source | G: before eligible model change; validate requested identity and existing assignment rules |
| PostModelSwitch | model/source | O/S: actual applied model and bounded context; no claim of transparent opaque-context transfer |
| ConfigChange | settings source | G: old generation checks the proposed digest; quarantine bypass is PRUN-003 |
| Setup | init/maintenance | O/S: explicit admitted setup with bounded context; not an install script |
| Notification | notification type | O/S: notification/context with provenance, not a developer command |
| FileChanged | admitted path/glob | O/S: change/add/unlink; `watchPaths` atomically replaces dynamic watches after access validation |
| CwdChanged | none | O/S: old/new workspace; validated dynamic watch update |
| DirectoryAdded | none | O: actual admitted directory and source, after workspace admission |
| MessageDisplay | none | S: profile-v1 delta/index/final message stream maps `displayContent` to display only; all partial/final output retains original bytes |
| Elicitation | MCP server name | G/S: form/URL request; accept/decline/cancel and bounded content; protected credentials require the explicit auth flow |
| ElicitationResult | MCP server name | G/S: response before server delivery; validate action/content and provenance; cannot invent a developer answer |

Dynamic watch results never widen file authority and retain static matcher paths.
SessionStart `reloadSkills` stages discovery, validation and explicit activation;
it cannot execute newly discovered code. `initialUserMessage`, `additionalContext`
and output replacements are plugin-origin even when an upstream field name says
user or system. Terminal notification sequences use a fixed safe allowlist and
never carry arbitrary terminal control bytes.

Custom WorktreeCreate code receives an admitted creation capability for a new
owned location, not general Git administration access. The host validates actual
Git worktree identity, starting dirty contents and ownership before accepting its
path. A non-Git checkout cannot satisfy the existing mutating-child Git contract;
that is an explicit policy conflict, not a silent alternative implementation.

## Ordering, deduplication and async behavior

Native transformers and final decision gates use declared priority, then managed,
bundled, user, project and private local-project scope, then package identity and declaration
index. Priority never changes authority or invalidates a higher-scope deny.
Default priority is zero. Source profiles preserve documented handler groups:
Claude concurrent groups start against the same captured candidate; the host
waits for every required decision before admission. Multiple unequal rewrites
from a concurrent group produce a conflict hold rather than an arbitrary winner.
Decision freshness and revalidation follow PRUN-001 even for source groups.

Deduplicate repeated references to the same canonical declaration within one
scope. Identical text in different packages/skills remains distinct. Each actual
invocation gets its own durable ID. A one-shot run reserves its identity before
effects and records consumption after success. Claude `once` is honored only in
skill frontmatter; known failed/blocked runs leave it eligible for a subsequent
matching event, not automatic replay of the same operation. Unknown effects hold
the reservation for reconciliation. In settings/agent frontmatter the source
profile identifies `once` as ignored. `async`, `asyncTimeout` and `asyncRewake` map to owned bounded
observer jobs. A late verdict cannot retroactively gate an operation. Rewake
creates attributed pending work under an existing allowance, not a fresh task
budget. The native timeout still applies where upstream would wait indefinitely.

## Connection ownership and bridge

Both native API adapters use the same host event implementation. External
backends retain their model loop but use a host-installed lifecycle relay, with
ambient user/project/backend plugins still disabled. This is an explicit exception
to disabling *all* backend hooks: only the trusted relay is registered there;
actual package code always runs in DemonCoder's managed runners.

| Event family | Native OpenAI/Anthropic | Claude subscription | Codex subscription |
|---|---|---|---|
| Skill expansion, task/child/team/worktree, settings, model selection, admitted workspace change, files, display, setup, MCP elicitation and notifications | Host owner | Same host owner; suppress duplicate relay observations of the same operation | Same host owner |
| Tool pre/permission/post/failure | Shared ToolExecutor | Shared ToolExecutor; correlate backend tool ID and host ID | Shared ToolExecutor; correlate dynamic-tool ID and host ID |
| Explicit batch | Host batch operation with declared members and settle barrier | Same operation; backend PostToolBatch callback separately describes actual backend batches | Same operation; no invented implicit backend grouping |
| Turn stop/failure, session start/end, cancellation | Host loop | Host session owner plus SDK lifecycle callbacks correlated once | Host session owner plus native lifecycle relay events correlated once |
| Manual and automatic compaction | Host compaction transaction and retained summary | Synchronous PreCompact/PostCompact SDK callbacks from actual backend operation | Synchronous registered PreCompact/PostCompact command relay at the actual core compaction barrier |

Claude integration registers host callback IDs in the existing initialize hook
map, equivalent to TypeScript SDK `options.hooks`. A `hook_callback` control
request carries callback ID/input/tool identity; the host returns that event's
typed output in the correlated control response. The inspected SDK release
exposes all 33 events. Use the qualified TypeScript/CLI protocol rather than
assuming the narrower Python SDK exposes every event.

Codex integration enables hooks only in an isolated generated configuration
containing the host relay command and no package handlers. The relay receives the
upstream event JSON on stdin, exchanges a correlated request/decision with the
host, and writes only the event-appropriate response. In the pinned Codex source,
`core/src/compact.rs` awaits `run_pre_compact_hooks` before compaction, and the
hook decoder uses `continue:false` for a stopped compaction. This is an actual
pre-action boundary, not a completion notification.

The host relay protocol is versioned and authenticated by a private inherited
channel/capability, never by an agent-supplied path or stdout text. Requests carry
connection/session/task ID, backend sequence, callback/event identity, operation
ID and deadline. The host records admission and returns a response bound to that
request and its final candidate key. Duplicate requests return the retained
decision without rerunning effects. Missing, stale or mismatched responses abort
the guarded transition. Observations carry an after-action marker and cannot be
used as pre-action acknowledgments.

Handler errors are converted to typed denials *inside the live relay*, not a
nonzero shell exit that upstream might treat as permission to continue. The
backend integration must also fail closed if the relay itself dies. Qualification
tests kill the relay at the exact waiting boundary and inspect actual backend
state. Stock command-hook failure handling is not assumed to satisfy this rule.
For the open Codex backend, the deliverable includes the narrowly scoped managed
bridge integration needed to treat relay loss as abort at the core wait; retain
its source revision, patch and build provenance. For Claude, use the SDK control
channel's correlated cancellation/connection termination and owner-supervised
backend lifetime, and prove the same boundary behavior before qualifying that
version. If a proprietary backend cannot meet it, record the demonstrated failure
for a developer decision; do not claim coverage or silently weaken the gate.

A backend version becomes selectable for this plugin contract only after the
real denial/allow/disconnect/timeout/relay-crash cases pass. This is a production
qualification test required by this commitment, not a claim those tests ran while
writing this specification. Existing ordinary connections keep their established
behavior while the draft remains unselected.

## Registered app binding

An app identifier is not a network address or an OAuth credential. Resolve it
through a developer-owned binding `(package source digest, app ID, service URL,
transport, account identity, allowed tools)`. The import screen displays package
identity and an available declared MCP endpoint as a proposal. The developer
confirms the service/account binding; absent or conflicting endpoints require
an explicit service selection. There is no assumed private OpenAI registry API.
Two names share a managed connection only after an explicit identical binding.

The fixed real connector case is the MIT-licensed pinned Supabase plugin in the
inventory. Bind its recorded app ID to its declared HTTPS MCP endpoint. Implement
MCP authorization discovery and browser authorization with PKCE, validate callback
state and service identity, and retain tokens in the host credential store.
Exercise authenticated tool discovery and a read-only project-list request under
a developer-authorized account; do not create, change or delete a project for
this smoke test. Account unavailability is visible missing evidence, not a mock
pass. Local fixture servers separately test rejected credentials, revocation,
wrong endpoint/account, redirects and expired tokens.

Bindings survive package updates only if the bound source/service identity is
still authorized; changed endpoints or requested access require a new binding
decision. Plugin code cannot enumerate unrelated account tokens. Configured apps
therefore work on every connection through the shared service layer without
pretending that subscription login grants access to an unrelated provider.
