# Reported test and access failures

Status: investigated 2026-09-07; original screenshot commands unavailable.

| Report | Reproduction/check and result | Disposition |
|---|---|---|
| PTY tests and read-only temporary storage | `cargo test --locked --test developer_access sweep_temporary_files_and_pty_work_inside_the_production_sandbox -- --nocapture` passes. The production executor runs Python, writes/reads a temporary file beneath its actual TMPDIR, opens a PTY and transfers bytes. | Not reproduced in the current production path. The earlier command may have hard-coded a read-only directory; without that command this is not established as its cause and is not called fixed. |
| Disabled Linux namespaces | `cargo test --locked --test developer_access sweep_nested_namespace_probe_retains_its_actual_disposition -- --nocapture` records `unshare --user --map-root-user -- /usr/bin/true`: exit 1, `unshare: write failed /proc/self/uid_map: Operation not permitted`. An ordinary subsequent sandbox command works. | Reproduced nested-user-namespace environment limitation. The sandbox stays enabled; no host fallback or broader access was introduced. This does not prove the exact screenshot test used the same command. |
| `fixture presentation failed` | `cargo test --locked --lib presentation_error_preserves_actual_failure_for_the_next_prompt -- --nocapture` passes. `src/native.rs` intentionally returns that error from a fixture presentation hook, then verifies the original failed tool result survives for the next prompt. | The known literal is an intentional failure stimulus in a passing regression, not a reproduced failing Rust test. The screenshot's exact failing command/output remains unavailable. |
| Blocked skill-file read | `cargo test --locked --test developer_access sweep_public_skill_read_preserves_private_canary -- --nocapture` passes for native read and Bash against a harmless outside `.agents/skills/fixture/SKILL.md`; the selected private credential remains denied. | General public skill reads work in this controlled path. The original path, symlink, ownership and access policy are unknown; no claim that a particular blocked original read was repaired. |

These checks use disposable directories, synthetic content and the production
ToolExecutor. They do not execute tasks against a real user repository or call a
live model. The other repository's daemon/worktree corrections remain unrelated.
The source-review inventory in `.cairn/backlog/verify-the-four-findings-in-the-supplied-source-review-screenshot.md`
is a different set of reports and remains open.
