# Clean up empty confinement overlay probe directories

Surfaced from: HOOK-006
Captured: 2026-09-10T13:44:42.545Z

DeveloperAccess creates demoncoder-confined temporary directories and probes overlay mounts. On 2026-09-10, /tmp exhausted all 1,048,576 inodes. Inspection found thousands of old confinement roots with empty work-N/work directories owned by the developer and mode 000. These directories prevent ordinary TempDir cleanup. A recovery removed 5,060 verified empty roots in two bounded passes with rmdir only and freed 75,900 inodes after checking process references. Add owned-directory cleanup that cannot follow symlinks or alter active mounts, and demonstrate that repeated overlay probes leave no temporary trees. Keep cleanup of user workspaces and task output outside this operation.
