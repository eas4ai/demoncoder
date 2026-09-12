# Connect private backend relays through pinned directory descriptors

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-005,HOOK-008,PRUN-001,PCOMP-003
Would be wrong if: A long protected path still prevents callback delivery, path replacement redirects a relay, or the repair weakens peer or token authentication.

## Decision

Actual Codex integration failed before model work because its protected Unix socket pathname exceeded the platform limit. The server already binds through a short descriptor alias, but the client attempted the full path. Open the client socket parent through the existing no-symlink descriptor-relative confinement helpers, then pin the exact socket inode with O_PATH and no-symlink resolution. Connect using a short /proc/self/fd alias owned by that client. Keep both descriptors alive through connection; do not shorten or relocate the protected directory, follow caller-provided symlinks, or replace the socket with a less protected transport. Preserve the exact socket basename, peer credential verification, private token and bounded frame contract. Demonstrate the original long-path failure, successful actual callback delivery with the repair, and refusal of symlinks or redirection in the parent or final socket component. Apply the repair to the shared transport and rerun existing compaction scenarios as well as ordinary callback cases. This repairs availability inside the current callback owner boundary and introduces no new execution authority.

## Realized by

- c8d06d72079412bdc13c909f31015102dfa3f749 Bind managed Codex Submit and Stop to durable lifecycle owners
