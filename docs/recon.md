# Demoncoder reconnaissance

Status: Observed 2026-09-07
Baseline: application commit `99e9ae8`; current CHAT receipts committed in `9eda67d`.
Scope: resume `handoff.md`, reconcile completed Creator identity, verify the current
chat commitment, and recover the already confirmed usability work from the wrong
repository. Observations describe the code; they do not silently change Agreed
requirements. The glossary in `docs/spec/glossary.md` remains the vocabulary.

## Exists

| Finding | Evidence |
|---|---|
| One Linux Rust terminal application, version 0.1.2, Rust 2024 / minimum Rust 1.95. The lockfile resolves Ratatui 0.30.2, Crossterm 0.29.0, Tokio 1.53.1, Reqwest 0.12.28 and Syntect 5.3.0. | `Cargo.toml:1`, `Cargo.lock`; technology choice in `docs/decisions/use-a-small-rust-terminal-host-with-explicit-session-owners.md`. |
| Startup selects a trusted workspace and named connection, then starts one session worker and the terminal with bounded command/event channels. This is not Suprnova's daemon/conductor architecture. | `src/main.rs:14`, `src/config.rs:242`, `src/session.rs:132`. |
| OpenAI/Anthropic feed the native loop; Codex and Claude own their external loops and use the host tool executor. Shared session events carry original tool outcomes and turn completion/failure. | `src/native.rs`, `src/adapters/mod.rs`, `src/adapters/codex.rs`, `src/adapters/claude.rs`, `src/session.rs:132`, `src/events.rs:14`; `docs/decisions/share-one-confined-executor-across-connection-tools.md`. |
| Creator identity and fourteen rules are implemented in all four adapters. The developer confirmed this work was already finished; reconciliation committed the existing diff, rather than selecting it as new scope. Tool-free Oracle sessions remain separate. | `src/adapters/creator.md:1`, `src/adapters/mod.rs:12`, `src/adapters/openai.rs:86`, `src/adapters/anthropic.rs:143`, `src/adapters/codex.rs:150`, `src/adapters/claude.rs:73`, `src/oracle.rs:44`, `src/tools.rs:168`; `.cairn/reviews/creator-identity.md`. |
| Settings are private TOML with CLI overrides; saving writes and synchronizes a temporary file, persists it, then synchronizes the parent. The optional event log appends attributed JSONL; that log is not durable session recovery. | `src/config.rs:242`, `src/startup.rs:373`, `src/startup.rs:430`, `src/events.rs:106`; `docs/proposals/developer-harness.md:42`. |
| The default executor mounts host files read-only, admits workspace writes and scratch/cache exceptions, shares networking and protects credential paths. It sets TMPDIR to writable session scratch. Explicit host access uses a separate Oracle policy. | `src/developer_access.rs:298`, `src/developer_access.rs:407`, `src/tools.rs:28`, `src/oracle.rs:44`; `docs/decisions/allow-developer-reads-and-networking-while-restricting-outside-writes.md`. |
| Chat groups original activity/results, bounds retained content, caches wrapping/highlighting, preserves scroll anchors and supports compact/full output. Mouse handling currently supports only wheel scrolling; Working is static. | `src/chat.rs`, `src/transcript.rs`, `src/highlight.rs`, `src/terminal.rs:105`, `src/terminal.rs:297`, `src/terminal.rs:371`; `tests/chat_presentation.py`, `tests/scrollback.py`. |
| Usage has optional input/output/cached/cost values, but no context-window, Git or child-session status fields. Unknown monetary cost is rendered when another usage dimension is present. | `src/events.rs:14`, `src/terminal.rs:205`, `tests/usage.py:102`. |
| Errors use anyhow context at boundaries; typed events report failed turns and original tool results. The Oracle rejects malformed/oversized responses and tool activity. | `src/main.rs:14`, `src/session.rs:132`, `src/events.rs:106`, `src/oracle.rs:44`; decision records for shared execution and explicit host access. |

## Documented

| Coverage | Evidence |
|---|---|
| Existing Agreed domains cover coding sessions, connections, startup, developer access/scrolling, output limits, usage visibility and chat presentation. Current commitment names CHAT-001 through CHAT-004. | `docs/spec/overview.md`, every domain in its map, `docs/spec/roadmap.md:3`, `docs/commitments/chat-presentation.md:3`. |
| README documents startup, terminal controls, settings, four tools, access modes, continuation, usage, extensions, limits and local/live verification. The broader harness document remains Proposed. | `README.md`, `docs/proposals/developer-harness.md:3`. |
| Product paths are divided into startup/configuration, session/native/adapters, tools/access/Oracle, events and terminal/chat/transcript/highlighting. Recorded decisions explain the toolkit, loop ownership, confinement, settings, output limits and display indexes. | `src/lib.rs`, `README.md` Implementation map, `docs/decisions/`. |
| Local verification uses Cargo suites and Python PTY/HTTP/backend fixtures. Shell mechanisms declare their requirements and inputs. Live drivers validate retained evidence by default; explicit run modes make live calls. | `.cairn/mechanisms/`, `scripts/check-chat-presentation.sh`, `scripts/check-connections.sh`, `scripts/check-coding-session.sh`, `README.md` Development and verification. |
| The inspected tracked tree has no CI workflow, container recipe or additional build-task file; the documented shell/Cargo commands own verification. No TODO/FIXME/todo!/unimplemented! markers were found in tracked source/tests. | Tracked-file and marker inspection at `9eda67d`; `Cargo.toml`, `scripts/`, `src/`, `tests/`. |
| History since the newest Agreed date covers output-limit, usage and chat delivery, then the broader proposal and Creator reconciliation. The active branch is docs/developer-harness-spec; local main and docs/public-readme also exist. Branches were inspected locally, without fetching. | Commits `74b2fd4`, `f2646e5`, `bb1324c`, `e0769be`, `40c25c8`, `99e9ae8`, `9eda67d`; `docs/spec/roadmap.md`. |

## Contradicted or missing

| Finding | Both sides and disposition |
|---|---|
| The handoff calls Creator work unfinished/unverified. The developer corrected that reading; its existing implementation is now committed and locally checked. | Original `handoff.md` Uncommitted creator work; code and checks in `.cairn/reviews/creator-identity.md`, commit `99e9ae8`. The stale in-progress marker was reconciled, not used to request the feature again. |
| The confirmed usability scope was written in the wrong repository and is absent from Demoncoder's current commitment. It is new requested behavior relative to the current CHAT contract, not evidence that all current CHAT requirements fail. | `handoff.md` Pending developer requests; Suprnova commit `6f433cb` and its NTU spec/commitment; `docs/commitments/chat-presentation.md:3`. Recovered mapping: `docs/proposals/normal-task-usability.md`; tracked in `.cairn/backlog/recover-the-confirmed-terminal-usability-sweep-in-demoncoder.md`. |
| Requested animation, scrollbar dragging, stable mouse selection and rich status are not delivered by this terminal. | Recovered request rows in `docs/proposals/normal-task-usability.md`; actual event loop/rendering at `src/terminal.rs:297` and `src/terminal.rs:371`. Proposed production checks and affected paths are listed in that mapping. |
| The later request to omit unknown money conflicts with the current formatter and its fixture expectation. It does not require converting unknown usage to zero. | Recovered NTU-011 mapping; `src/terminal.rs:205`, `tests/usage.py:119`; existing `docs/spec/usage-display.md` and `docs/spec/connections.md` distinguish unknown usage. Change the test expectation with the selected behavior, not to manufacture a pass. |
| README says two Rust entry points are intentionally ignored; this run has three. | `README.md` Local checks; `cargo test --locked` result and `.cairn/reviews/chat-presentation.md` Verification and delivery (which already names all three). Documentation correction remains recorded. |
| The repository's working-agreement copy predates the installed skill template. | Original `AGENTS.md`; `/existing-project` Stage 3 and the sibling new-project working-agreement template. Updated that copy verbatim during this adoption; no project-specific preamble was present in the file. |

## Retained findings and unverified claims

No earlier `docs/recon.md` existed. The existing backlog is retained by reference;
this report does not close its findings merely because they are outside the scope.

| Item | Evidence and remaining limit |
|---|---|
| General Markdown prose/file links and the broader development-experience inventory remain future work. | `.cairn/backlog/render-markdown-prose-and-file-links-in-the-chat.md`, `.cairn/backlog/improve-the-development-experience.md`, `docs/proposals/developer-harness.md`. |
| Hash-anchored edits remain a preference awaiting selected requirements. | `.cairn/backlog/prefer-hash-anchored-edits-using-omp-as-the-reference.md`; current exact-text edit shape in `src/tools.rs:84`. |
| Four existing source-review reports remain open: host Unix socket visibility, queue backpressure, Anthropic argument accumulation and per-chunk UTF-8 decoding. | `.cairn/backlog/verify-the-four-findings-in-the-supplied-source-review-screenshot.md`. Current seams: `src/developer_access.rs:320`, awaited UI sends in `src/terminal.rs:324`, `src/adapters/anthropic.rs:199`, `src/tools.rs:627`. The helper at `src/adapters/anthropic.rs:277` has a 4 MiB bound, so the argument path must be traced/reproduced before repeating the unbounded claim. No exploit or correction check was run here. |
| The four reported PTY/namespace/fixture-presentation/skill-read symptoms are separate from those source-review reports. Exact originating commands are unavailable. | `handoff.md` Pending developer requests; recovered investigation row in `docs/proposals/normal-task-usability.md`. Passing local Cargo/PTY checks does not identify those original failures. |
| Suprnova's daemon and worktree fixes are not Demoncoder evidence. The other repository also has later commits beyond the handoff's last observed commit. | `handoff.md` Mistaken changes; inspected Suprnova history ends at `60bb3aa`, with `b635373` after the handoff's `0a772f4`; Demoncoder entry point `src/main.rs:14`. No changes were made there. |
| Fresh live-provider compatibility and installed-binary delivery were not re-established during reconciliation. | Historical `.cairn/evidence/live/`, `.cairn/evidence/live-oracle/` and `.cairn/reviews/chat-presentation.md`; new checks listed below are local. |
| Graph discovery is available after indexing Demoncoder. Ripwire's root task map included ignored reference projects; its source-only test gate lacked Python-driver edges and returned exit 4, with 31 impacted symbols and zero mapped tests. | `src/`, ignored `reference/` in `.gitignore`, direct drivers under `tests/`; `.cairn/reviews/creator-identity.md` records the limitation and executed checks. Graph counts do not prove coverage. |

## Verification performed

| Command | Actual result and record |
|---|---|
| `cargo test --locked` | Passed: 55 tests, 3 ignored. `.cairn/reviews/creator-identity.md`. |
| `cargo build --locked` | Passed. `.cairn/reviews/creator-identity.md`. |
| `python3 tests/continuation.py --ownership` | Passed all four adapters after completed/cancelled turns and wrong resumed-session rejection for both external backends. `tests/continuation.py`, `tests/continuation_fixture.py`; `.cairn/reviews/creator-identity.md`. |
| `cairn check CHAT-001` | Recorded pass for CHAT-001 through CHAT-004. `.cairn/evidence/CHAT-001/20260907T062350676Z` and matching requirement receipt paths; retained output records 23 library tests and six terminal test methods. |
| `git diff --check` | Passed for the Creator diff before its commit. `.cairn/reviews/creator-identity.md`. |
| Installed Cairn `scripts/spec-lint.mjs docs/spec` | Failed with eight existing findings, detailed below; no pass claimed. Sources are the named spec blocks. |

Spec lint findings: CHAT-002 combines two obligations in one sentence;
OUTPUT-002 does so in two sentences; START-003 does so in one sentence.
`coding-session.md:59` and `:74` use undeclared `/tmp` host paths;
`connections.md:37` and `startup.md:17` use undeclared `~/.demoncoder` paths.
These are structure/declaration findings in existing Agreed text. They have not
been silently rewritten or presented as new runtime failures.

## Work preparation

The recovered usability mapping identifies its source, affected modules and tests,
acceptance cases, repository-specific exclusions and unresolved interpretation
points in `docs/proposals/normal-task-usability.md`. The existing glossary and
Agreed spec set remain the contract; no Observed requirement was promoted by this
report. The current CHAT review is refreshed in `.cairn/reviews/chat-presentation.md`.
The roadmap's next commitment remains a developer selection, rather than a
consequence of discovering a proposed feature.
