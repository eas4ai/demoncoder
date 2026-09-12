# Bind native session HTTP and MCP hooks to original lifetime authority

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-008,PLUG-002,PLUG-003,PRUN-001
Would be wrong if: A transport borrows task funding, a reused service changes lifetime or resets its limits, a held or stale call has effects or delivers a pass, or service cleanup outlives the native shutdown boundary.
History: Preserve the prior native lifetime and explicit session funding decisions and the approved Codex source applicability correction. This adds native transport execution without weakening source qualification or changing remote-call accounting policy.

## Decision

Enable synchronous native SessionStart and SessionEnd HTTP/MCP handlers only from the original explicit SessionHooks grant and actual native lifetime. HTTP and MCP use the existing time and service-local limits; they do not spend model slots or Agent inspection-tool slots. Resolve preparation, queued waits, startup, requests and delivery against the exact original budget and the current admitted occurrence, with no task fallback. Preserve command-only observation without an explicit grant and the already implemented model accounting.

Bind a managed MCP connection to its original task owner or original live native session lifetime, never merely to a package configuration. For session services, keep the original cumulative grant deadline and service-local call count across startup and end. Every new occurrence still needs its own live admission, matching service configuration, workspace, generation, snapshot and credential authority. A startup occurrence finishing does not end the actual host session. Add only the in-memory lifetime tracking and finalization needed to distinguish that interval from shutdown, cancellation, replacement, recovery or deserialized history. Original host lifetimes marked complete in the operation ledger are recorded facts, not proof that their terminal observation is unavailable.

Reuse a connection only with unchanged authority and retained-view identity. Preserve the existing refusal when a stdio service's immutable snapshot no longer matches; snapshot readmission and public activation remain later integration work. Do not reset or revive a revoked, failed or cross-lifetime service object. Task replacement, stop, acceptance or archive cannot revoke or replenish an independent session grant or transfer the service to another owner. Check the live invoking owner and exact funding after durable transitions and immediately before transport effects and delivery. Retain unknown outcomes and original settlement after cancellation without granting new work.

Idle services must not hold the startup observation drain open. On actual final shutdown, revoke the original service lifetime, terminate and reap local processes and descendants, close connections, and drain their owned cleanup within the existing five-second whole end boundary and eight-second application reservation. Startup keeps its thirty-second whole boundary. Cancellation of the invoking session observation or actual host lifetime, recovery, credential revocation, expiry and abandoned host execution must also dispose of owned service work. Cancelling an unrelated task does not cancel the independent session service. Keep cleanup ownership until actual teardown, including when futures are dropped or connection locks are busy. Existing synchronous kernel-I/O timing limits remain explicit.

Use compiled failing and corrected controls for actual no-prompt native startup/end HTTP, Streamable HTTP MCP and confined stdio MCP; unchanged-view start/end reuse, retained call caps, queued expiry, exact owner mismatches, task independence, missing/exhausted grants, held recovery, cancellation, late replies and observed process/connection cleanup. Native MCP end must actually execute. Codex SessionEnd MCP remains accepted-but-skipped according to its frozen source contract; do not fabricate backend lifecycle events or infer live-provider qualification. Keep asynchronous session-command support separate, and leave the complete lifecycle/package commitment open.

## Realized by

(none yet: recorded, not built)
