# Demoncoder handoff

Written: 2026-09-07

## Repository boundary — read first

The intended project is `/home/shawn/workspace2/demoncoder`.
Pass that absolute directory as `workdir` on every shell call. The session's
inherited default directory was suprnova-coder and caused substantial work to
be committed in the wrong repository. Do not trust the default directory.

The developer explicitly corrected the project and confirmed this path.
Another agent is working on Cairn, not on the unfinished demoncoder adapter
changes. The developer's latest instruction was to write this handoff.

Do not resume development in suprnova-coder. Do not automatically revert,
cherry-pick, or port its changes. Its architecture and contracts differ from
Demoncoder's, and cleanup there has not been authorized.

## Observed Demoncoder state

- Branch: `docs/developer-harness-spec`.
- HEAD: `40c25c8` — `Draft the developer harness feature specification`.
- Package version: `0.1.2`; Rust application with ratatui/crossterm and Tokio.
- `docs/spec/roadmap.md` still names `chat-presentation` as Current.
- `docs/proposals/developer-harness.md` contains the broader feature draft.
- `AGENTS.md` requires the Cairn workflow. Read its instructions, the glossary,
  `docs/spec/overview.md` (the keystone), roadmap, commitment, and applicable
  decisions before implementation.

Last `cairn wake` verdict:

```text
Resolvable: reconcile implement creator-identity at 40c25c8ed09e7ff5a03f5565beb9994d5079bf58
  .cairn/in-progress names an unfinished action; finish or abandon it, then remove the record
```

The existing marker says:

```text
action: implement
target: creator-identity
base: 40c25c8ed09e7ff5a03f5565beb9994d5079bf58
started: 2026-09-07T05:21:12Z
```

## Uncommitted creator work to reconcile

These changes were already present when the correct repository was opened:

- `src/adapters/creator.md` (untracked): shared Creator identity and fourteen
  production coding rules. The agent identifies as Demoncoder while answering
  accurately about the underlying provider/model. Rule 13 applies a self-audit
  before completing every todo item and before delivery.
- `src/adapters/mod.rs`: includes that shared prompt.
- `src/adapters/anthropic.rs`: adds the prompt through `system`.
- `src/adapters/openai.rs`: adds it through `instructions`.
- `src/adapters/codex.rs`: adds it through `developerInstructions` on thread
  start/resume.
- `src/adapters/claude.rs`: adds `--append-system-prompt`.
- Each adapter conditions the prompt on having tools, intending to leave the
  tool-free Oracle's separate role intact. Review whether that condition
  accurately distinguishes the roles in every supported path.
- `tests/continuation.py` and `tests/continuation_fixture.py`: assert Creator
  identity reaches all four provider/backend request paths.
- `README.md`: describes the identity and shared rules.
- `tests/__pycache__/` is also untracked; it is generated test output.

No implementation changes, commits, builds, or behavioral tests were performed
in Demoncoder after correcting the path. The existing diff was inspected only.
Do not describe these changes as verified. This handoff is the only new file
written in Demoncoder during the path-correction work.

Relevant existing commands include `cargo test --locked`,
`cargo build --locked`, and `python3 tests/continuation.py --ownership`.
Inspect each script's arguments and behavior before running it. In particular,
`scripts/check-connections.sh` includes live-provider checks; it is not merely
an offline adapter test. Cairn evidence must be collected against committed
inputs through the named mechanism, following the local working agreement.

## Pending developer requests

The developer had confirmed a reliability and terminal usability sweep before
the repository mistake was discovered. Reconcile these requests with actual
Demoncoder source and its specification before naming a commitment. The NTU
requirements mistakenly created in suprnova-coder are not Demoncoder contracts.

- Animate the working indicator during silent model waits, including planning
  and review where those phases actually exist. Preserve useful active intent.
- Support dragging the transcript scrollbar and selecting text with the mouse
  for copying. New output must not steal a manually scrolled position or alter
  selected text.
- Omit monetary cost when it is unknown or only partly priced. Known zero is
  still a valid value.
- Show a compact status strip in the developer's requested order: model;
  context used and remaining capacity; dirty-file count and active branch;
  diff additions/deletions; active subagents; existing token counts.
- Context occupancy is the current prompt/context, not cumulative billed input.
  Do not invent an unknown context capacity; label estimates. Do not imply
  subagent execution already exists merely by displaying a count.
- Investigate four screenshot reports separately: PTY tests failing on
  read-only temporary storage, disabled Linux namespaces, a Rust test reporting
  `fixture presentation failed`, and a blocked skill-file read. Exact originating
  commands and run are unavailable in the currently inspected Demoncoder files.
  Record evidence limitations; do not call a symptom fixed without a reproducer
  and correction check. Do not weaken confinement to get a passing check.
- Use normal bounded tasks in disposable repositories. The developer rejected
  live execution against real user repositories and did not request endurance
  testing.
- Subagent execution and provider-selected native search were discussed as
  subsequent work. Do not silently add them to the usability sweep.

Earlier work also concerned the broader developer-harness proposal and an
`existing-project` reconnaissance pass. Inspect the proposal and current source;
that reconnaissance was not completed. Do not infer that proposed features are
implemented or import suprnova-coder's daemon/run/worktree design into this app.

## Mistaken changes in suprnova-coder

The developer was told plainly that changes were made there. They remain in
`/home/shawn/workspace2/suprnova-coder`; no rollback was performed. The mistaken
work included local commits for:

- A normal-task reliability and terminal usability specification, roadmap,
  commitment, backlog entries, mechanism reviews, and evidence receipts.
- A test proving attached task clients receive terminal failure without polling.
- A bounded foreground Tokio runtime shutdown, with a blocked-reader subprocess
  and real SIGTERM test.
- Read-only repository Git metadata mounts for task Bash, with sealed capability
  paths and inspection/replacement tests.

Useful commit anchors from that work:

| Commit | Change |
|---|---|
| `6f433cb` | Named the wrong-repository reliability/usability commitment. |
| `6463a40` | Terminal-delivery test and mechanism. |
| `418e1a8` | Daemon runtime teardown bound. |
| `405410b` | Sealed read-only Git metadata mounts. |
| `0a772f4` | Last observed evidence commit after those changes. |

There were additional decision and receipt commits between these anchors.
Inspect history before any separately authorized cleanup; do not revert a broad
range without checking ownership. No push was performed during the mistaken
operations described here.

The checks in that repository did demonstrate terminal delivery, a baseline
shutdown hang followed by roughly one-second corrected exit, interrupted-task
recovery, and read-only Git inspection. They prove nothing about Demoncoder.
The last wrong-repository verdict was `Resolvable: declare NTU-004`; the overall
usability commitment was not completed. A previous live disposable exercise
also used suprnova-coder and is not evidence of a Demoncoder failure.

## Resume checklist

1. Confirm the absolute working directory is Demoncoder and read `AGENTS.md`.
2. Run `cairn wake` and reconcile the existing `creator-identity` action without
   discarding the pending adapter changes or treating them as already verified.
3. Keep a todo list with exactly one item in progress. Finish code, meaningful
   verification, and self-audit before marking an item complete.
4. Follow the current repository verdict and developer-confirmed scope. Resolve
   the mismatch between the pending requests and the old roadmap explicitly.
5. Prefer codebase-memory MCP for code discovery. It previously returned
   `Transport closed`; if still unavailable, use local source discovery and
   report only observations actually established here.
6. Keep suprnova-coder and the other agent's Cairn work outside this task.

## Reconciliation update — 2026-09-07

The developer corrected this handoff in the resumed session: Creator identity
was already finished. Its existing pending changes were preserved, tested and
committed as `99e9ae8`; `.cairn/reviews/creator-identity.md` records the results.
The stale in-progress marker was cleared. Fresh CHAT receipts were committed as
`9eda67d`. Do not select Creator identity again as unfinished feature work.

The developer also directed inspection of suprnova-coder. Its commit `6f433cb`
contains the previously confirmed usability spec and commitment. The Demoncoder
mapping is now `docs/proposals/normal-task-usability.md`, and `docs/recon.md`
records current source evidence and outstanding findings. The other repository
was inspected read-only; its implementation and receipts were not imported.
