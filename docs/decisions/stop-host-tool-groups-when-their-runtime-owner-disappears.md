# Stop host tool groups when their runtime owner disappears

Level: Judged
Decided by: Codex
Rests on: VERIFY-005 VERIFY-006 CODE-010
Would be wrong if: Host tools survive loss of the runtime lifetime pipe, cancellation leaves descendants active, or the supervisor changes host access policy

## Decision

A disposable crash test showed a host Bash descendant still active two seconds after DemonCoder was killed. Supervise host Bash in a separate process of the current executable, with a private stdin lifetime pipe held only by the runtime. Launch the actual Bash process after a bounded nonblocking handshake. The supervisor acts as a Linux child subreaper: it adopts orphaned descendants, including processes that detach into new sessions. EOF, unexpected control data and ordinary completion trigger repeated direct-child kill and reap operations. Direct children remain owned until reaped, preventing PID reuse from redirecting cleanup signals. Parent cancellation drops the lifetime pipe instead of killing the supervisor before it can clean up. This changes process ownership, not filesystem confinement or Oracle admission. Confined Bash retains bubblewrap parent-death cleanup.

## Realized by

(none yet: recorded, not built)
