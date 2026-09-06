# CODE-002 mechanism review

The PTY driver runs the production binary with each of the four adapters.
A controlled peer requests read, write, edit, and bash in order and awaits
each actual result before deciding the next request. The peer never writes
the answer file. The driver creates a random seed, then checks the retained
read value, four distinct call identities, actual final file bytes, and a
successful verification command containing the prompt's unique marker.
The native Anthropic case splits tool JSON across streaming deltas.

The executor test separately runs a meaningful failing Python assertion,
then an edit and passing assertion. Both exit statuses remain in the event
stream. A second test tries harmless outside-canary overwrites through
parent paths, symlinks, hard links, and a hook-modified request; all are denied.
These two tests passed. They supplement the per-connection PTY cycle and do
not establish the whole CODE-007 or CODE-008 contract.

Failure demonstration: `python3 tests/terminal_session.py --tools
--fault-wrong-edit` makes the controlled peer request an incorrect edit,
while retaining the correct command assertion and expected final bytes.
The four connection cases fail. The corrected invocation without the fault
passes all four cases. This demonstrates that a model's final response or
successful transport alone cannot make the mechanism pass.

An initial executor test exposed a failed descriptor mount: bubblewrap
could not resolve the parent's proc descriptor after isolation. Passing the
authorized directory as an inherited descriptor through `--bind-fd` fixed
the mount; no path-based mount fallback was added.

Limits: these peers exercise real adapter transports and real tools, not
live model services. Full streaming checkpoints, steering, cancellation,
continued context, external built-in restrictions, and authentication still
need their separately named checks. The live smoke requirement remains open.
