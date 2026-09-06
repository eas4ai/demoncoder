# Startup mechanism review

Requirements: START-001, START-002, START-003

The mechanism builds the production binary and runs the existing onboarding and
configuration checks before the new startup regressions. Its declarations include
all source, tests, manifests, and the script; a changed prerequisite cannot leave
successful START results from an earlier part of the command.

## Failure demonstrations and corrected cases

The installed binary accepted help and uppercase version, but lowercase `-v`
exited with status 2. The first version-alias implementation attempted to mutate
Clap's generated version argument and panicked in the information test. A second
implementation made a boolean field required during normal startup. An optional
field with the Version action now passes all five information forms and normal
startup. The tests assert matching version output and no home-directory writes.

The reported owned home directory had mode 0775. The prior settings-lock code
refused it before onboarding. The disposable reproduction now reaches the prompt,
changes that directory to 0700, and preserves an existing canary. A separate
saved-settings fixture verifies connection fields, a synthetic credential, and the
default connection survive a trust update; the credential is absent from output.

The workspace test checks both omitted and explicit workspace selection. Each
case drives a read/write/edit/Bash cycle through the real host tools, checks all
four successful tool results and the resulting file, and compares saved trust
with the canonical project path. This confirms the existing default behavior.

The symlink refusal fixture uses a mode-0700 target, so it cannot pass merely
because the older group-write check would reject the target. It requires a failed
startup without any target file or permission changes. A custom mode-0775 parent
must also be refused without chmod or settings/lock-file changes.

## Limits

The foreign-owner branch is inspected in source: descriptor ownership must match
the current UID before chmod. These unprivileged terminal fixtures do not create
a directory owned by another UID. The descriptor-bound permission change avoids
redirecting chmod through a replaced final path; this review does not claim the
entire existing settings transaction is descriptor-relative. No new live provider
request is needed to test these startup paths; earlier provider receipts retain
their original revision and limits.
