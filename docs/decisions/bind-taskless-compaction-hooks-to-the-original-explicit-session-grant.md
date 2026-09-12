# Bind taskless compaction hooks to the original explicit session grant

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-007,HOOK-008,PRUN-001,PRUN-002
Would be wrong if: A compaction hook borrows another owner's funding, an exhausted task falls back to session funding, a summary spends hook resources, or service reuse resets limits or permits a stale occurrence.
History: Extend the existing original native session model and transport ownership decisions to actual taskless compaction occurrences. Preserve their grant, accounting, lifetime, reuse and revocation rules; no new allowance or ledger is introduced.

## Decision

Let PreCompact and PostCompact hooks around an actually taskless native compaction use the existing explicit session hook grant. Capture the original live NativeSession lifetime in the typed compaction receipt before work. Admission must follow the exact live lifecycle occurrence to its compaction and then to that captured lifetime, with matching session, identity, workspace, policy and unchanged original grant deadline. An existing task or child keeps its own owner and allowance; missing, exhausted or invalid task funding never falls back to SessionHooks. Missing, exhausted, replaced, recovered or ended session authority cannot authorize a hook.

Prompt and Agent handlers spend the existing session grant's model and inspection resources. HTTP and MCP retain their existing time and service-local limits. Command behavior keeps its existing synchronous and asynchronous rules. The single tool-free summary request remains ordinary Unallocated model work with its own exact usage receipt; it cannot spend the session hook grant or reserve deferred Creator context.

A managed MCP service retains its existing original native-session owner, service fingerprint, cumulative grant deadline and service-local call count across startup, successive compactions and end when the authority and retained view remain valid. Each request still validates its own exact unfinished lifecycle receipt, compaction transaction, reserved hook, admission key, budget and snapshot. Ending or cancelling one compaction invalidates its requests without creating a fresh service allowance. Revoke or close services under existing lifetime, grant, authority and snapshot rules; do not revive a failed service or add a parallel counter ledger.

Prove delivery from an explicit grant and refusal without one; task and child ownership precedence; exact summary usage; cancelled, expired and replaced owners; and repeated-compaction MCP reuse with retained call limits. This is part of the existing batch and compaction implementation and retains its fresh specification and quality reviews.

## Realized by

993d1a259574f0de8c7b15d9a11dbc8fafbf3915
