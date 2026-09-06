# Session ownership check

The continuation driver now checks one declared owner per session and one
start/result pair for each completed create, read, edit, and verification
operation. All four connections run both completed-turn and cancelled-turn
continuation. The subscription peers retain their own conversation state,
require the original resume identity, and report whether a process resumed.
The check compares those facts with the actual retained tool results.

The ownership mode also returns an unrelated identity during Codex or
Claude resumption, then queues a harmless write. Each adapter must report
its identity error before admitting that tool. The canary remains absent
and the preceding completed source stays unchanged. Normal resumption and
both mismatched-identity cases passed.

Temporarily removing Codex's resumed-thread equality check made the
ownership check fail its expected resume-error assertion. The separate
event-thread check still rejected the queued request, so the fault did
not produce a file effect. Restoring the equality check made all cases
pass. This demonstrates both the intended early check and the surviving
independent check; it does not claim that every malformed backend message
or arbitrary duplicate request is covered.
