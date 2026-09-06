# Secure the owned default settings directory during setup

Level: Judged
Decided by: Codex
Rests on: START-003 and the developer-reported mode-0775 home directory failure
Would be wrong if: A foreign-owned or symlinked directory is changed, a custom shared parent is silently chmodded, or existing settings are replaced during permission repair

## Decision

When setup needs a settings lock, open the parent directory without following its final symlink, require current-user ownership, and use that descriptor to set the default home settings directory to mode 0700. Explicit custom configuration parents retain the existing non-writable-by-others requirement and an actionable error. Preserve all existing directory contents. Verify both the repaired home case and rejected symlink/custom cases using disposable fixtures.

## Realized by

(none yet: recorded, not built)
