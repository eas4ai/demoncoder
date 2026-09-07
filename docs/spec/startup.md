# Startup fixes

Status: Agreed 2026-09-06
Prefix: START
Host paths: ~/.demoncoder

The developer reported the existing home settings directory failure and requested
help, lowercase version, and the launch directory as the default workspace.

[START-001] The CLI MUST provide --help and -h, and report its version through -v, -V, and --version without starting setup or a provider session.
Falsifier: An information flag errors, starts setup, changes home settings, or the version spellings disagree.
Mechanism: startup; invoke the real binary with isolated home settings and inspect exit codes, output, and absence of created files.

[START-002] When --workspace is omitted, the session MUST use the process launch directory. An explicit --workspace MUST override it.
Falsifier: Saved trust or tool effects refer to a different workspace, or an explicit workspace is ignored.
Mechanism: startup; launch the real terminal in temporary projects, save trust, and execute a fixture tool request to check its actual working directory.

[START-003] Guided startup MUST accommodate an existing user-owned default ~/.demoncoder directory by securing it to mode 0700 before updating settings. It MUST reject a symlink or foreign-owned settings directory. It MUST NOT silently change permissions on the parent of an explicit custom configuration.
Falsifier: An owned mode-0775 default directory blocks setup, its existing files change during directory repair, a symlink target is changed, ownership is ignored, or a custom shared parent is silently chmodded.
Mechanism: startup; drive the production setup in disposable homes with existing directories and canaries, and check repaired permissions, preserved files, refusals, and normal session startup.

This repair concerns the settings directory when setup or a trust update needs to
write settings. Credentials remain in private files; custom directories retain
an actionable permission error. The coding loop and tool-access policy do not
change. The earlier first-session evidence remains historical evidence for its
recorded implementation; this commitment verifies the startup changes.
