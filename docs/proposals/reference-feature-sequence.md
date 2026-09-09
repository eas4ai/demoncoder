# Reference-informed feature sequence

Status: Proposed requirements, 2026-09-09.

The developer requested implementation of the features identified in the
reference reviews, in logical order. This document makes the proposed scope
and its checks concrete. It does not describe these features as implemented.
The existing release-followups commitment is complete.

## Delivery order

Each row is a separate deliverable with its own Cairn commitment and review.

| Order | Deliverable | Observable result and proof |
|---|---|---|
| 1 | Managed language services | Rust and TypeScript navigation, hover/types and diagnostics through the shared executor on all four connections. Prove correct locations, current document synchronization, explicit unavailable states and confined process cleanup. |
| 2 | Recoverable tool-output shaping | Each connection receives bounded useful output with references to retained originals. Prove failures and useful structured payloads survive; source reads and recovered originals are not repeatedly shortened. Compare captured requests and original receipts. |
| 3 | Native context retention | Native API sessions select context while retaining the task, corrections, unresolved failures and evidence references. Prove continuation across forced context pressure and compaction. Codex and Claude retain ownership of their history and compaction; the shared output layer benefits them before admission. |
| 4 | Session-history recall | Search this application's authorized workspace history with bounded results and source references. Prove retrieval of original records, exclusion of other workspaces and private credentials, and clear attribution of historical claims. External application history requires separately selected roots. |
| 5 | Diagnostic-backed lessons | Relate new, persistent and resolved diagnostics to actual edits and verification evidence. Feed the existing candidate/outcome/lesson workflow. Prove repeated diagnostics do not become a clean result and an unverified correction cannot become an enabled lesson. |

Source maps, mutation serialization, artifact protection and cancellation
patterns are implementation techniques within these deliverables, where needed.
They do not authorize a second orchestration system. General extension loading,
automatic external-history import, checkpoint restoration and speculative
learning systems are outside this sequence.

## First commitment: managed language services

Proposed name: `managed-language-services`. Proposed requirement prefix: `LSP`.
The implementation extends the existing shared executor and session owner.
Project opening remains usable without an installed language server.

[LSP-001] The application MUST support explicitly enabled, installed Rust and
TypeScript language servers scoped to the selected workspace. It MUST report
server availability, initialization failure and supported operations accurately.
Falsifier: Opening a project silently executes a project-provided server command,
a missing server prevents ordinary coding, or an uninitialized server is shown
as ready.
Mechanism: Exercise disabled, missing, failed and successful initialization through
production configuration and a controlled stdio server; run installed-server
smoke cases for both languages.

[LSP-002] Definition, references, hover/type information and document diagnostics
MUST be accessible through the shared tool boundary on all four connections.
Results MUST retain source paths and positions, distinguish empty results from
unavailable operations, and make bounds or truncation explicit.
Falsifier: A connection cannot invoke an operation, a Unicode position resolves
to the wrong symbol, an unsupported request appears to succeed with no results,
or a large response silently loses results.
Mechanism: Drive every adapter with controlled tool requests against a temporary
project and server, including non-ASCII text, empty results, unsupported methods
and oversized responses. Verify Rust and TypeScript navigation with real servers.

[LSP-003] Queries and post-edit diagnostics MUST identify the source revision
they describe. Successful write/edit operations MUST synchronize changed text.
Late diagnostics MUST NOT overwrite a newer revision or be reported as current.
Absence of a fresh response MUST remain pending, stale or unavailable.
Falsifier: Delayed errors for an old document replace current diagnostics, a
successful edit queries old text, or a timeout is reported as zero errors.
Mechanism: Deliberately reorder diagnostic notifications across two edits; test
versioned and unversioned notifications, external file changes, server restart,
timeouts and the corrected current response. Conservatively label freshness
unknown when the protocol cannot establish it.

[LSP-004] Language-server execution and file access MUST obey the session's
existing access policy. A server MUST NOT apply edits, execute requested commands,
read protected files or broaden its workspace merely by sending a protocol
request. Read-only and child policies MUST retain their existing restrictions.
Falsifier: A fixture server reads a protected canary, writes outside its allowed
workspace, or causes an unsolicited workspace edit or command to execute.
Mechanism: Launch a harmless adversarial server under production confinement;
exercise protected paths, outside URIs and server-initiated requests. Inspect
actual canary effects, including child and review-only sessions.

[LSP-005] Server startup, requests, retained messages and shutdown MUST be
bounded. Cancellation and session shutdown MUST stop owned execution and leave
the terminal responsive. Protocol errors MUST produce actionable failures.
Falsifier: A stalled server blocks cancellation, a malformed or oversized frame
causes unbounded accumulation, Unicode framing loses a message, or a server child
continues after its owner exits.
Mechanism: Split UTF-8 frames at every byte boundary; inject malformed lengths,
oversized messages, a stalled request and a child process. Observe bounded errors,
cancellation, owner exit and cleanup through the production process boundary.

[LSP-006] Post-edit feedback MUST preserve the actual mutation result and
separately report diagnostic state. Repeated errors MUST remain visible as
existing errors even if duplicate notifications are suppressed. Diagnostics
MUST NOT establish that tests passed or that work was accepted.
Falsifier: A diagnostics timeout hides a completed edit, repeating an error turns
it into a clean result, or an empty diagnostic set marks verification passed.
Mechanism: Edit a failing fixture twice, preserve the persistent error, then
correct it and observe a fresh clean result. Repeat with a server failure after
the file write and inspect both the mutation receipt and verification state.

Rename, code-action application and multi-file edits follow in a separately
specified slice after revision checks and mutation admission are established.
This first slice never applies a server-supplied patch.

## Completion evidence

The commitment requires passing production-path protocol and confinement cases,
installed Rust and TypeScript server smoke cases, all-four-connection adapter
coverage, applicable regression gates, documentation and a final review.
Controlled provider cases establish adapter behavior; they must not be described
as live authenticated provider evidence. Record exactly which transports ran.

For context work, measure retained input size and turns before compaction on
fixed workloads. Also test whether corrections, failures and task constraints
survive. The user's observed longer and more coherent OMP/PCN sessions motivate
the work; exponential improvement is not an acceptance claim.

## Selection

Confirming this first slice selects LSP-001 through LSP-006 and their falsifiers
for the next commitment. The agent then records them in `docs/spec/`, creates
the commitment, advances the roadmap and implements under Cairn. Later rows
retain their order but require concrete requirements before their implementation.

If this isn't clear, ask me to explain it another way before you decide.
