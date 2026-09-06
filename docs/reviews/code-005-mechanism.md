# CODE-005 mechanism review

Reviewed 2026-09-06 against the agreed cancellation requirement and falsifier.

The production binary receives a prompt and Escape through a pseudo-terminal
in a fresh temporary Git repository. Each of the four connections runs two
cases: a held provider response and a Bash operation with a child writing a
heartbeat file. The external provider peers also launch an actual helper
process. Before cancellation the driver requires visible waiting output and,
for subprocess cases, repeated heartbeat activity and observed descendants.

Two seconds after sending Escape, every observed owned subprocess must be
terminated. PID start times distinguish the observed process from a later
reuse of its number. Zombies count as terminated because they cannot work.
The heartbeat must remain unchanged during a further observation interval.
Native streaming requests must reach server-side EOF within two seconds.
The retained outcome must say cancelled. A new unique prompt must then
produce a response in the same open terminal. The driver cleans up its own
remaining fixture processes when a case fails.

Failure demonstrations executed:

- Against the preceding executable, native provider and all four Bash
  cases passed. Both external provider cases failed because a backend helper
  remained alive beyond the two-second limit (exit 1).
- After process-group cleanup, all eight cases passed (exit 0).
- `python3 tests/cancellation.py --fault-drop-cancel` withholds Escape.
  Both native HTTP requests remained open and all six subprocess cases
  retained active processes. All eight cases failed (exit 1).

An additional Rust test covers explicit cleanup and owner drop after the
backend leader exits while a helper remains. Repeated explicit cleanup must
also succeed without signaling a stale group ID. These use real disposable
processes; they do not execute destructive repository probes.

Limits: protocol peers establish local cancellation behavior, not a remote
service's computation or billing outcome after disconnect. The process
group covers ordinary inherited helpers; confinement of backend tools and
their ability to create independent processes remains CODE-007 work. The
next-prompt check establishes that the terminal remains usable. Preservation
of conversation context and backend reattachment belong to CODE-006. No live
provider claim is made by these checks.
