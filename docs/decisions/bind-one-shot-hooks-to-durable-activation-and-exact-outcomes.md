# Bind one-shot hooks to durable activation and exact outcomes

Level: Judged
Decided by: agent
Rests on: HOOK-008,HOOK-011,PCOMP-002,PRUN-001
Would be wrong if: A new turn, restart or changed registration clears one-shot consumption or an unresolved effect, or a hook is consumed without its exact successful outcome.

## Decision

Implement synchronous one-shot execution first in the existing durable session hook ledger, before extending it to owned asynchronous observers. Record explicit host skill activation separately from immutable package/service generation; only explicit reinvocation creates a new activation epoch. Bind each reservation to the canonical declaration, scope, session and activation and the exact event invocation. Reserve atomically before hook effects, consume only after a known successful outcome, leave a known failed or blocked attempt eligible for a later matching event, and retain unresolved effects across epochs until explicit reconciliation. Record skips as references to consumption evidence, never invented executions or replayed effects. Preserve final-candidate admission and original post-tool evidence. Claude settings and agent once fields remain ignored; native and Claude skill declarations follow their explicit activation semantics. Actual pinned Claude probes show synchronous success consumption, known-failure retry and explicit skill reactivation; upstream async consumes at launch, but the agreed host contract deliberately requires actual success. This prerequisite does not discharge async, source activation, public management or full lifecycle delivery.

Reconciliation is a separate host operation for one exact unresolved attempt.
The developer may attest that the attempt was unsuccessful, with a retained
reason. That fact permits eligibility on a later matching event; it never rewrites
the original unknown outcome as observed success, approves guarded work or replays
the old operation. Generic recovery acknowledgment cannot supply this attestation.
The public developer control will use this same operation when management is wired.
An active post-tool lifecycle also retains ownership until proposal settlement,
including while a later handler group runs. Reconciliation must wait for both
that lifecycle and any runner cleanup to end; a group finishing is insufficient.

For local packages, capture source identity from the imported package's canonical
root, separately from its display name, code digest and directory inode. Canonical
aliases refer to one source; separate roots with the same manifest name remain
distinct. Replacing files or the directory at the same source cannot reset an
unknown attempt. Package runner construction verifies the binding against that
captured source. Unpackaged native host handlers use a separate host namespace.

## Realized by

- 7343d1a0d7122a2af2e303e159a0fcdd403a8a67 Persist one-shot hook activation and exact recovery
