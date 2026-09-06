# Share one confined executor across connection tools

Level: Judged
Decided by: Codex
Rests on: Agreed CODE-002 and CODE-007; external backends retain loop ownership
Would be wrong if: A backend can bypass admission, Bash reaches unauthorized files or credentials, or tool results lose their call identity

## Decision

Expose read, write, edit, and bash through one Rust executor. Native model adapters request those tools through a shared small loop. External backends call the executor through their supported custom-tool protocols. Validate final typed arguments, use Linux openat2 for native file access, and run Bash through mandatory bubblewrap with isolated system mounts and a clean environment. Retain actual results before presentation. Verify actual temporary-repository effects for each connection; fixture evidence alone does not establish live backend equivalence.

## Realized by

- 9d85df10cd0e5c151e23cf29635fb0ce08d60e54 Route four connection tool cycles through the shared executor

This establishes controlled transport and tool-cycle evidence. Complete
external tool confinement and the remaining session controls await their
separately named requirement checks.
