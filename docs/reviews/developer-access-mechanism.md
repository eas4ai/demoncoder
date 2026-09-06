# Developer access mechanism review

Requirements: USABLE-002, USABLE-003, USABLE-004

## Reproductions

A read-only traversal of the actual DemonCoder repository reproduced the old
validator's first refusal at target/debug/demoncoder (two hard links). Other
normal build outputs and a reference-tree symlink also met its blanket refusal.
The old error did not identify the path.

The initial production-tool regression suite failed six cases: ordinary build
links/reference symlinks, an unrelated command beside an outside alias, native
outside documentation/Git reads, installed Cargo, Git inspection, and a controlled
network endpoint. The existing credential refusal case passed. The tests used
only disposable repositories and harmless canaries.

The new implementation passes those cases, including a second Git staging/commit
command after .git already exists. Both native and Bash reads preserve the outside
documentation; native mutation and direct/symlink/hard-link Bash writes leave the
outside canary unchanged. Failures name the affected paths.

Two additional regressions exposed and then verified corrections for native writes
to an explicitly selected private configuration and Bash reads of nested private
settings. A descriptor check confirms bubblewrap consumes the host directory FD;
it is closed before the task command starts.

## Cache and credential boundaries

Binding all existing caches writable required protecting enough hard links to
exceed the process argument limit. The final implementation uses session-owned
copy-on-write cache storage, reading the installed cache where supported. A safe
true-command probe selects that capability before any task command executes.
Without it, a private empty cache keeps installed Cargo executable shims readable.
A forced-fallback test starts the installed fixture tool, writes cache data, and
checks that the host cache did not change.

The four-adapter installed-backend driver now distinguishes ordinary outside
source from private credentials. All four adapters passed outside documentation
and machine-standard reads, permitted project work, protected credential canaries,
and blocked outside writes. The actual installed Codex and Claude processes use
local fixture endpoints in these tests; these checks do not make paid live model
requests. Backend-owned substitute tools remain unavailable.

The command removes inherited provider credentials, masks known credential stores
and selected secret configuration, and exposes machine instruction files and
skill/plugin code read-only. Native mutations remain rooted through openat2.
Ordinary Git commands can update the selected repository's metadata. A raw native
write to .git remains refused; Git commands are the supported mutation path.

## Limits

The filesystem boundary does not defend against another host process maliciously
replacing entries while admission inspects them. Cache changes persist for the
session and are removed on close. Missing service authentication remains distinct
from network permission. Current remote, CI, vulnerability, and Cairn claims need
their own observed results; a working shell alone does not establish them.
