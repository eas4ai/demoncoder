# Deny host Unix sockets in confined Bash while preserving IP networking

Level: Consequential
Decided by: Codex
Rests on: REL-001; existing developer networking and explicit host-execution contracts
Would be wrong if: Confined commands can reach host Unix control services, ordinary TCP or UDP networking stops working, the filter can be changed by a command, or explicit host mode is silently restricted

## Decision

Attach a syscall filter only to confined Bash. Deny creation of Unix sockets, deny Unix datagram socketpairs, and retain anonymous stream socketpairs. Deny io_uring creation and unsupported syscall ABIs so they cannot bypass the filter. Keep TCP and UDP networking, native file access, existing workspace and cache mounts, and explicit host mode. This also disables filesystem and abstract Unix sockets created entirely inside confined commands, including local Docker, database and SSH-agent sockets; developers needing those use the existing explicit host mode. Read-only mounts cannot prevent socket connections, pathname masking misses dynamic and abstract sockets, and network namespace isolation would remove ordinary networking. Keep the filter in an unlinked parent-owned file and pass it through a trusted argv-only launcher while retaining the pinned workspace descriptor. Verify disposable pathname and abstract services, datagram bypass denial, stream socketpair and IP networking success, unsupported ABI rejection, and the existing access regressions.

## Realized by

- e4246a3f02e189db4274ca2294675260fd840e8a Block host Unix socket access from confined Bash
