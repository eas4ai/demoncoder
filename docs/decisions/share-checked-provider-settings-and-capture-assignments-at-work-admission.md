# Share checked provider settings and capture assignments at work admission

Level: Judged
Decided by: Codex
Rests on: SET-001 SET-002 SET-003 SET-004 SET-005 SET-006 SET-007 SET-008
Would be wrong if: A settings change alters admitted work, grants authority, loses a private update, or labels unchecked authentication as valid

## Decision

Use one settings editor for onboarding and the live terminal. Run bounded, cancellable provider checks outside the input loop; API model listing validates the selected key, Codex uses account/read and model/list without creating a thread, and Claude uses its non-coding auth status command with explicitly labeled supported model choices. Store role overrides separately from Creator so inheritance remains live, retain existing connections and legacy Oracle overrides, and publish private settings under the existing lock with conflict detection. Resolve and capture connections at task or role admission, preserving explicit CLI and assignment overrides and the authority chosen at launch. Preserve admitted task, queued child and recovery identity; changing an opaque backend begins a distinct context and never claims to transfer its session. Exercise the real terminal, actual fixture requests, persistence failures and existing execution boundaries; retain negative demonstrations before claiming checks work.

## Realized by

638ab058b0ea208e8bb47344880db2d7d9edb21e Add checked provider setup and live agent model assignments

- `src/settings/` supplies shared provider checks, the keyboard editor and private
  persistence with revision checks. `src/terminal/settings_panel.rs` keeps live
  editing independent of the running session.
- `src/workflow/`, `src/subagents/manager.rs` and `src/oracle.rs` resolve role
  defaults at admission and retain original work identity. Explicit launch and
  assignment selections keep their existing precedence.
- `tests/provider_agent_settings.py`, `tests/live_settings.py` and
  `tests/role_settings.py` drive the production terminal and inspect actual
  fixture requests, durable records, private saves and recovery. The mechanism
  also installs and repeats these tests against the release binary.

## Reference and protocol evidence

The original pi selector named in the specification informed the list/submenu
interaction. It is not claimed to be the requested oh-my-pi checkout. No reference
code was copied; this implementation uses the existing Ratatui terminal.

Provider checks follow the installed Codex app-server protocol and its official
[app-server documentation](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md),
plus Claude's [CLI reference](https://code.claude.com/docs/en/cli-reference) and
[model configuration](https://code.claude.com/docs/en/model-config). Codex checks
use account/read and paginated model/list only. Claude auth status does not take
a coding prompt; its model aliases are labeled supported choices, not account
entitlements. Probe fixtures reject any attempted coding action during discovery.

## Failure demonstrations

The unchanged numeric-setup binary fails the actual checkbox PTY checkpoint.
The corrected flow passes provider tests that deliberately reject keys and login,
return malformed or unavailable catalogs, delay discovery and cancel checks.
Synthetic reflected credentials must not reach terminal output or event logs.

The original Reviewer timing behavior also fails a held-correction request case:
it uses reviewer-old after Settings saved reviewer-new. Resolving at the actual
review invocation makes that same case pass while preserving the task's Creator.
Private-save tests inject competing raw-file edits and a held settings lock; both
must preserve the prior runtime model and exact file contents. Repaired saves,
queued work and interrupted recovery have separate positive cases.
