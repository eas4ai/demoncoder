# First coding session

Status: Agreed 2026-09-06
Requirements: CODE-001, CODE-002, CODE-003, CODE-004, CODE-005, CODE-006, CODE-007, CODE-008, CODE-009, CODE-010, CONN-001, CONN-002, CONN-003, CONN-004, CONN-005, CONN-006

## Deliverable

The developer opens DemonCoder in a selected workspace, selects a supported
connection, and completes coding work through a responsive terminal session. First start
guides setup and project trust. Explicit --yolo selects host execution with
an Oracle guard for outside access and approved temporary scratch space.
The session streams text and tools, accepts a correction, cancels running
work, and continues with the preceding turn's context and changes.

The first native loop exposes read, write, edit, and bash. Typed hooks extend
its behavior under runtime admission controls. API connections use this loop.
Codex app-server and Claude headless sessions have explicit external owners
and pass the same observable session checks. All four requested connections
belong to this commitment; one working API path cannot establish the others.

This is larger than a single-provider prototype. Work can establish the
native path before integrating the external backends, but Done requires the
complete requirement set. Advanced subagent orchestration and durable crash
recovery remain the later roadmap commitments.

## Records and formats

- Contract: docs/spec/coding-session.md and docs/spec/connections.md.
- Design rationale: docs/spec.md and docs/decisions/.
- Mechanisms: .cairn/mechanisms/coding-session and .cairn/mechanisms/connections.
- Evidence: Cairn receipts and retained output under .cairn/evidence/.
- Specification review: docs/reviews/first-coding-session-spec.md.
- Implementation review: .cairn/reviews/first-coding-session.md, written only after the work is examined.

The mechanisms report individual requirement results using Cairn's
per-requirement format. A missing result remains unverified. Failed setup,
missing credentials, and unavailable CLI binaries are identified as such;
they are not reported as successful checks or as proof of a behavioral
violation.

## Planned mechanisms

The command declarations name future check drivers. Those drivers and the
application do not exist yet. No runtime pass is claimed by this setup.

`bash scripts/check-coding-session.sh` will build the declared application
configuration and drive the production executable through a pseudo-terminal.
Controlled provider and CLI fixtures pause at explicit checkpoints, so the
driver can inspect rendering, input, steering, tool effects, and cancellation.
Real tool execution uses temporary repositories and harmless canaries.

`bash scripts/check-connections.sh` will exercise adapter registration,
authentication selection, backend-session identity, capability errors, and
usage reporting. It will also require a redacted live smoke record for each
initial connection from the current implementation: a two-turn coding task
with observable tool effects. A protocol fixture does not replace that live
transport evidence. Authentication prerequisites and externally unavailable
services are recorded as unresolved evidence rather than skipped passes.

Each initial connection is tested against the shared CODE cases. The check
drivers aggregate those cases by requirement and fail any requirement whose
required connection case fails. They preserve which adapter and scenario
produced each result.

The declarations currently include only the tracked design inputs that
exist. Before running a mechanism against implementation work, the agent
adds every source, test, build, lockfile, fixture, configuration, and check
driver that can affect its result to the declaration in the same change.
Evidence directories are not mechanism inputs. A pending driver is not a
complete mechanism and cannot establish a behavioral pass.

## Failure demonstrations

When each mechanism is built, the agent safely demonstrates that it detects
its intended violation and passes the corrected case. Examples include
buffered output that fails the streaming checkpoint, a hook-mutated denied
path whose marker never appears, a deliberately failing repository check,
and an adapter that selects the wrong synthetic authentication route.
These demonstrations use fixtures and controlled faults, not destructive
probes. The implementation review records what the demonstrations could not
establish.

## Done when

Every named requirement is Agreed and has current passing evidence against
committed inputs. All four connections have executed their required cases.
The live transport evidence is present. The implementation review has no
open findings. The developer can reproduce the documented coding session.

Decisions about exact Rust components, terminal toolkit, and adapter
protocol versions are recorded before their implementation. Any backend
limitation that would change an agreed behavior returns as a concrete
decision; it cannot silently weaken the contract.
