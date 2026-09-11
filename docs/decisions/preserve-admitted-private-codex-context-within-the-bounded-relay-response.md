# Preserve admitted private Codex context within the bounded relay response

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-005,HOOK-009,HOOK-011,PRUN-001
Would be wrong if: Accepted private context is still shortened before model delivery, oversized responses are admitted, or normal upstream and compaction behavior changes.

## Decision

The actual full-payload host test found that a valid60KiB private Submit contribution was replaced by a roughly10KiB preview and an inaccessible spill-file path. The private handler inherited the normal2,500-token preview limit; the earlier source test checked only a marker and missed lost content. Disable that second preview/spill stage only on private ordinary handlers using the existing AdditionalContextLimit zero setting. Keep the strict64KiB complete serialized response bound, required acknowledgment, host lifecycle context limits, deadlines and all failure behavior unchanged. Normal upstream handlers retain their configured/default preview behavior and compaction remains unchanged. Strengthen the real-command source boundary test and actual model-peer qualification to compare the complete admitted context, including its middle, rather than a marker alone; keep65,537-byte rejection as the violating control. Rebuild and rebind source, patch and binary provenance, rerun source and host regression checks, and obtain independent reviews before replacing the qualified artifact. This repairs the previously intended full-context delivery inside existing finite bounds; it adds no unbounded output channel.

## Realized by

- c8d06d72079412bdc13c909f31015102dfa3f749 Bind managed Codex Submit and Stop to durable lifecycle owners
