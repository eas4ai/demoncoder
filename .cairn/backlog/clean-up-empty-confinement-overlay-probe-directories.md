# Clean up empty confinement overlay probe directories

Surfaced from: HOOK-006
Captured: 2026-09-10T13:44:42.545Z

DeveloperAccess creates demoncoder-confined temporary directories and probes overlay mounts. On 2026-09-10, /tmp exhausted all 1,048,576 inodes. Inspection found thousands of old confinement roots with empty work-N/work directories owned by the developer and mode 000. These directories prevent ordinary TempDir cleanup. A recovery removed 5,060 verified empty roots in two bounded passes with rmdir only and freed 75,900 inodes after checking process references. Add owned-directory cleanup that cannot follow symlinks or alter active mounts, and demonstrate that repeated overlay probes leave no temporary trees. Keep cleanup of user workspaces and task output outside this operation.

Maintenance on 2026-09-12: at the developer's request, a one-time cleanup checked
process and mount references, then used descriptor-pinned, no-follow traversal
and rmdir only on owned probe roots older than one hour. It removed 46,027 roots
and 609,428 empty directories, accounting for about 2.09 GiB of allocated directory
blocks. Files and symlinks were not removed; 3,182 recent roots and 296 roots that
could not be emptied were retained. All reported removed roots were verified absent.

The cleanup's synthetic control preserved a regular file, a symlink and its outside
target while removing empty directories, including mode-000 leaves. The plan and
result are retained in `/home/shawn/demoncoder-check-tmp/stale-probe-cleanup-{plan,result}-20260912T191240084516Z.json`.
This maintenance does not repair automatic probe cleanup; the production follow-up
above remains open.
