# Assignable subagents review

commitment: assignable-subagents
commit: 1dc7b4a8343ce66b6115026ef912238a2da6971c
findings:
  - none: component reviews approved; final committed-candidate review remains pending
Status: in progress

## Strict tool component specification review

Candidate component 94cd704af806fee84f8712d1c5b09aefb39cd048 passed ten strict
boundary tests. Independent review reproduced an omitted path: inside a private
user/mount namespace, a disposable outside directory was bind-mounted beneath
the worktree. Native read returned its canary; native write and Bash changed it.
The outside file changed even though worktree-only policy was selected. Native
resolution lacks NO_XDEV and shell inspection traverses mounts before binding
the root. Reject mount crossings at both tool boundaries; preparation alone is
not a sufficient invariant. The candidate was not changed during review.

### Strict tool repair demonstration

The new private-namespace regression first demonstrated nine native and shell
operations reaching bind-mounted outside canaries. Strict resolution now uses
NO_XDEV for existing and newly created files, and the shell inspects every entry
through the pinned root before exposing it. The corrected eleven-case strict
suite passed independently in specification review. Ordinary developer/tool
regressions passed eighteen tests with one existing ignored entry point.
Clippy and formatting passed in the component worktree. Independent quality
review approved the source and tests without claiming another execution run.

The component intentionally supplies no network and no home-installed tools.
Only minimal read-only system runtime dependencies are exposed; pre-existing
hard links block shell admission. These limits must be visible in documentation.

## Worktree component review

Specification review at 3ad5bf0003d3402ac460366db26fc847d3b8cb19 independently
passed fourteen integration tests after repairs for readonly baseline directories,
nested owned-file ancestor creation and permission increases before patch writes.
The dirty baseline, index preservation, ownership, conflicts and Git hook/filter
suppression passed the component review.

A cancellation followup found that materialize writes files synchronously in one
future poll, with a sixty-second loop deadline and no deadline in the final mode
loop. Capture immediately afterward is also synchronous. There is no detached
copy task, but dropping or aborting the future cannot interrupt that poll. Git
descendant tests do not demonstrate cancellation during copying. Repair this
separately and demonstrate responsiveness and cancellation before merging.

Quality review also reproduced a configured diff-prefix escape: local
diff.srcPrefix/dstPrefix settings can change generated patch paths. Application
can then change a matching unowned file before the final snapshot rejects the
result. Pin canonical patch prefixes and demonstrate that a configured prefix
cannot redirect an owned change. This finding is recorded before its repair.

### Worktree repair demonstrations

Both specification and quality review approved component
663dd3ebb3f8097b7e59cc4498585bbd9af78904. Each independently passed fifteen
integration tests, three cancellation tests and two scanner tests. Copy and
metadata loops now yield between entries and bounded writes; read-only background
scans receive cancellation flags. The 56 MiB copy test demonstrates timer
responsiveness, cancellation under two seconds and no later destination changes.
This is not a hard real-time promise for kernel-stalled filesystem calls.

Canonical diff generation pins a/ and b/ prefixes, disables relative/color
transformations and ignores configured order files. The prefix regression first
reproduced an unowned overwrite, then passed with the unowned sentinel unchanged.
The approved component was merged as eddb36188c10c2a442799c72fbd709ea5a113bc0.

## Manager specification review

Read-only review found three concrete gaps. Archive clears the shared allocation,
so ordinary parent work after abandon can bypass its former limits, and resume
can create a fresh allowance. Integration lacks the active-count check used by
assignment and validation, allowing a third active job at a limit of two. Finally,
turn-completion/error publication ignores cancellation while waiting for terminal
capacity; with background children this can leave effects running. Record these
findings before their separate repairs and demonstrate each violating sequence.

Quality review found a second cancellation path: launch allocation notices use
lossy advisory delivery. If that update is dropped, the terminal can believe no
child is active after the parent stops and ignore Escape or Ctrl-C. Cancellation
must not depend on presentation state. Add a dropped-notice regression before
repairing this path.

### Manager repair demonstrations

The production fixture first demonstrated an actual parent file write beyond the
tool limit after abandon, and a third active integration at a limit of two. Both
now pass: abandonment retains the allocation and integration checks capacity
before preparation and within the durable transition. Missing saved allocation
on resume is refused rather than replaced. Specification re-review approved
these repairs.

The held-lifecycle regression first timed out with background work still active;
it now passes because every cancellation reaches background cleanup. The dropped
allocation-notice regression also first failed. Escape and Ctrl-C now send the
owner cancellation regardless of displayed activity. Quality re-review
independently passed this regression and approved the repair. The actual
four-connection heartbeat tests passed individual, idle-parent and shutdown
cancellation with effects stopped within two seconds.

Production tests additionally passed all four coding tools on every child
connection, hostile home/Git/path operations under a yolo parent, failed checks,
review findings, stale child files, parent conflicts, unowned changes, acceptance
invalidation with retained prior evidence, native-call/backend-invocation/tool
limits, concurrency and deadlines. Killing the owner during preparation, child
execution and integration intent retained uncertain state and produced no replay
on resume. These are editing-time tests; Cairn receipts and the final installed
candidate remain separate completion gates.
