# Assignable subagents review

commitment: assignable-subagents
commit: ff4ef017426dc96a4c86f9095ffef1989a4fd4a9
findings:
  - none: all recorded findings repaired and verified
Status: complete

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

## Final regression finding

On the committed candidate, tests/output_limits.py failed its production
continuation test: a truncated Anthropic response leaves the runtime uncertain
and rejects the next prompt until /reconcile, although the response admitted no
tool call. The original output-limit contract requires visible truncation and
a usable next prompt. This finding is recorded before repair; investigate the
model-admission completion path without weakening the interruption boundary.
Full Rust tests, Clippy, formatting and all six verification production cases
passed. Terminal regression drivers through startup passed. Final review and
installed verification remain incomplete.

### Truncation repair demonstration

The production continuation test failed twice before the change. Native response
errors returned before finish_model, leaving a live admission even though native
model requests do not execute coding tools. The loop now settles a returned
response before propagating its error. Cancellation still exits before settlement.
The complete output-limit terminal suite and twelve Rust output-limit cases pass,
as do production recovery VERIFY-006, all-four cancellation, native interruption
unit tests, queue and streaming tests, formatting and Clippy. Independent source
review approved the distinction; unknown usage remains unknown.

Ripwire edit-check found no signature mismatch. Its quality delta flags recent
churn and four lines of added explanation in run_turn; no abstraction or behavior
change is warranted to reduce that history metric. The test gate names eight
Rust/driver paths and dynamic-dispatch gaps; controlled Rust and terminal checks
cover the relevant behavior. The live Oracle validator was run and rejected its
old receipt as stale; no paid-provider run was made and no live pass is claimed.

## Final candidate review and release audit

Reviewed the assembled committed manager, developer-control wrapper, native
response loop, durable admission path, strict policy and integration boundary.
Attacks covered dropped UI notices, held lifecycle publication, abandon followed
by work, a third concurrent integration, failed and stale validation, configured
Git patch prefixes, bind-mounted canaries, interrupted preparation and integration,
and returned provider errors. Recorded findings above were fixed in separate
commits and demonstrated to fail before their corrections. No open finding remains.

The final cargo test --locked run passed all executed tests (six explicit fixture
or live entry points remain ignored by that command). Formatting and strict
all-target Clippy passed. Existing verification production cases VERIFY-001
through VERIFY-006 passed; recovery and cancellation were rerun after the native
error repair. Controlled coding-session cases passed through host access; its
script then failed the live Oracle receipt validator because that receipt is
stale. This is not a current live-provider pass. No paid requests were made.

Additional terminal drivers passed queue responsiveness, registry, mouse and
clipboard behavior, chat presentation, scrollback, status, usage, configuration,
startup, output limits, authentication, capability rejection, inherited descriptors
and the usability contract. The current seven-requirement Cairn mechanism passed
after the native repair. Plan completion records reuse the existing private-store
failure and old-record tests rather than claiming duplicate child-store machinery.

Installed with cargo install --path . --locked. The executable at
/home/shawn/.cargo/bin/demoncoder and target/release/demoncoder both have SHA-256
b2bb346c5ccf8759b5f913f2ae25bc98108e117ebe204518697ee0bc6da3c492.
With DEMONCODER_TEST_BINARY selecting that installed executable, all seven
assignable-subagents production requirements and all three output-limit terminal
tests passed. The four transports are controlled fixtures exercising real adapter,
terminal, filesystem and process behavior; external service behavior is not proved.

### Production standard self-audit

1. Mapped the agreed requirements, existing session owners and shared runtime
   before implementation; the glossary and contract remain authoritative.
2. Kept changes within the selected commitment and its inherited dependencies.
3. Reused adapters, tool execution, durable storage and review instead of adding
   another agent loop, database or daemon.
4. Validated assignment/configuration boundaries and preserved default four-tool
   sessions and backward-compatible durable records.
5. Retained original errors and results; unknown usage stays unknown. Synthetic
   credentials and canaries exercised protection without exposing real secrets.
6. Enforced child confinement independently of parent flags and Oracle decisions;
   filesystem and shell escape demonstrations now fail closed.
7. Persisted intent and admission before effects, retained interrupted identities
   and prevented automatic replay or fabricated integration success.
8. Bounded admissions, deadlines, active jobs, output and retained state; tested
   cancellation under output pressure and during worktree copying.
9. Maintained one in-progress task; marked plan items complete after their code
   and applicable verification finished.
10. Ran production success/failure paths, Rust checks and affected terminal drivers.
    Recorded the stale live receipt separately from controlled passes.
11. Completion claims are limited to executed evidence and inspected code; the
    roadmap still contains later commitments.
12. Applied the developer corrections for all four connection types, genuine Git
    worktrees and mandatory child confinement.
13. Reviewed every rule and resolved observed defects before delivery. No further
    revision is indicated by current evidence or source review.
14. Documented controls and limits in plain language, including no child network
    or home toolchains, unknown backend usage and non-atomic external-writer races.
