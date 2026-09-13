# Admitted workspace replacement

Status: Design recorded; implementation pending. This is a bounded prerequisite
of the existing skills/plugins/hooks commitment. Complete lifecycle dispatch is
the sole task in progress. No application qualification is claimed.

## Contract and code discovery

[The decision](../decisions/own-admitted-workspace-replacement-through-the-original-session.md)
owns a developer-selected idle workspace replacement, preserves the original
allowance and relevant conversation, and records actual post-change observations.
PCOMP-003's ownership table assigns admitted workspace changes to the host on
all four connections. CwdChanged is an observation after access validation; there
is no PreWorkspaceChange event. Shell cd alone is not the host operation.

The current application has no public live workspace-change route. Root authority
is retained by ToolExecutor descriptors, workflow/runtime state, Settings controls,
Manager child admission, providers and the terminal. Original native lifetime
validation is also root-bound. A path setter would leave stale authority. Discovery
and a separate contract assessment are retained under
`/home/shawn/demoncoder-check-tmp/workspace-transition-discovery.md` and
`workspace-contract-review.md`. These are read-only assessments, not tests.
The parent resolved their suggested control/accepted-task UX choices within the
existing specification; no narrower commitment or scope change was created.

## Pinned Claude control observations

The exact installed Claude 2.1.267 executable has SHA-256
`0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0`.
The fetched SDK 0.3.267 archive matches the profile's existing hash
`4177598847f37041aadcfe1a922a0153e2e58495100521233fc587e1c0839485`;
its sdk.d.ts also matches the recorded profile. No source code was copied into
the deliverable. The archive and inspected SDK files remain in scratch
`workspace-transition-sdk-0.3.267/`.

The source probes used disposable homes, directories, synthetic authentication,
empty setting sources, disabled tools, strict empty MCP configuration and a
controlled loopback HTTP peer. No provider prompt was sent and the peer recorded
zero requests. This is not live-provider or packet-level network isolation proof.

- `register_repo_root` registers a strict child of cwd or a launch-time additional
  directory. Self, outside and duplicate requests fail without another callback.
  The settled run receives two genuine DirectoryAdded callbacks after successful
  registration responses. A deny/block/continue-false observer reply cannot undo
  registration; the later duplicate remains refused.
- `set_cwd` returns transport success even for nested rejection or needs_trust.
  Explicit accepted trust produces actual cwd changes, independently observed
  through `/proc/<pid>/cwd` and relative read_file returning unique B contents.
  Same-root requests report changed:false. Two settled changes and a canary
  variant produce no CwdChanged callback. This is limited negative evidence for
  these controls, not a claim that every Claude CwdChanged trigger is absent.
- Embedded source inspection identifies a separate shell-tracking callback path
  and shows that set_cwd rehomes settings/hooks/skills, memory and plugins/MCP.
  That static inspection is not execution evidence for every branch. Canaries
  stayed inactive with setting sources disabled, but arbitrary package loading
  and destination memory reads were not qualified. The application decision
  therefore uses controlled replacement rather than live set_cwd.

The initial matrix `workspace-claude-source-matrix-20260913T142413841096Z` has
14 completed steps, but lacks settled trusted-transition observations. The
14-step settled matrix is `workspace-claude-source-settled-20260913T142434087128Z`.
The canary run `workspace-claude-source-canary-20260913T142541380126Z` retains a
harness failure on a legitimate commands_changed system message; this is not a
backend failure. The corrected 14-step run is
`workspace-claude-source-canary-fixed-20260913T142602070061Z`. Raw inputs, scripts,
sent/observed messages, results, canaries, stderr and cleanup results are retained.
All processes exited after cleanup. Stderr files are empty; the probe's file sink
was not byte-capped, a harness limitation.

The parent verified retained script hashes, completed/failed run distinctions,
actual root changes and relative reads for settled cases, duplicate/no-op behavior,
callback counts and no submitted model prompts. The parent audit is
`workspace-source-parent-audit-20260913T142949890317Z.json`, SHA-256
`9aa8b828fe942293d2e50c38aa018af49d037b6f32f555aeef06116adbbbfbf8`.
It binds every retained regular file in all four runs. The first parent audit
incorrectly applied trusted-transition assertions to the initial matrix and
failed before writing; the corrected audit retains that distinction.

The contract assessment confirms that truthful host old/new facts may use the
supported Claude wire format, like the existing host ConfigChange mapping.
Such delivery must retain host provenance and never claim an SDK callback.
Missing set_cwd callbacks do not themselves require changing the profile or
requesting a scope exception. Required conversation continuity and source-specific
output effects remain requirements. Dynamic watch installation and DirectoryAdded
are still subsequent work; parsed/unapplied watches cannot count as completion.

## Verification still required

Application implementation, failing behavioral controls, focused regressions,
independent specification and quality reviews, full regression and exact candidate
integrity checks remain required. Source probes above establish none of those.

Retained workspace-transition-discovery.md SHA-256: `0f76b34c64ee78e79c07160474b456fe64eb549eb54f3f04e23074e662e7030d`.

Retained workspace-contract-review.md SHA-256: `db449262f18efef54e1a69b8052e7ca383684f4c625e5d6623b079c2d5a19ed1`.

Retained workspace-claude-source-report.md SHA-256: `fbd885d8ab711111a12f9eaec3f13f6c9d798bfc338d990b9f3f42e478a31ca0`.

Retained workspace-claude-source-audit.json SHA-256: `ee24e08a1d59233cc13aad0ea580610b5bb6e49714a5993162c49c540c3d5a4f`.
