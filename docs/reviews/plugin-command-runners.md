# Confined command runner review

Status: Approved by independent specification and quality reviews. S1, S2 and the staging descriptor finding are closed. This prerequisite does not complete the plugin commitment.

## Scope

The candidate adds host-selected PreToolUse command execution, immutable snapshot materialization, explicit file grants, bounded protocol streams, and owned process cleanup through the existing admission and receipt paths. Commands run confined in ordinary and host modes. This prerequisite denies network access; declared network grants, configured handler timeouts, other runner types, remaining lifecycle events, activation and public management remain in the complete implementation plan.

The base is `7cb920bd9791992734bf5289429561c3aacc2591`. The final candidate is pinned by `/tmp/plugin-command-runner-fd-manifest.json`, SHA-256 `f207d6f19d37218297deacc5281f9522d925064c6dbb8d3d43baa0ef5f057975`. The parent independently verified all 24 source and 40 artifact hashes. The full initial handoff is `/tmp/plugin-command-runner-handoff.md`; correction handoffs are `/tmp/plugin-command-runner-s12-handoff.md` and `/tmp/plugin-command-runner-alias-handoff.md`.

## Closed specification findings

### S1: Private files under live write grants

The first independent review found that live parent-directory grants exposed default-private descendants. The shared WorktreeAccess traversal did not apply `export_policy::private_path`. Synthetic local controls reproduced four disclosures across root/containing-directory grants and both access modes.

The correction adds an extra exclusion predicate to the existing traversal. Hook preparation supplies the existing private-path policy; ordinary Bash keeps its established predicate. Direct private grants fail before execution, parent grants retain private masks, and public `.env.example` controls remain usable. The new regression failed before correction and passed afterward: `/tmp/plugin-runner-s1-red.log` and `/tmp/plugin-runner-s1-green.log`.

### S2: Relative credentials and launch-time aliases

The first review also found that resolving raw relative credential paths against process cwd lost their workspace-relative interpretation. Two independent controls reproduced disclosure from the retained snapshot. The correction expands the existing host `private_roots` policy before absolutizing it, freezes that meaning at executor construction, and shares lexical and physical exclusions between capture and hook preparation. A later cwd change does not reinterpret authority. Ordinary no-plan construction remains compatible.

The added tests exposed a dangling-alias case. The nearest-ancestor resolver previously treated a failed canonicalization as an absent path component even when the component was an unresolved symlink. It now holds when the alias target cannot be established; a valid directory alias with an absent leaf remains supported. The original required-absence regression failed before correction and passed afterward: `/tmp/plugin-runner-s2-dangling-red.log` and `/tmp/plugin-runner-s2-dangling-green.log`.

Re-review then found that launch preparation still ignored failed full-path canonicalization after capture. Changed final aliases and changed directory aliases with absent credential leaves could expose retained snapshot bytes. The final freshness check held the guarded write only after the command had received those bytes. The independent adjacent suite reported one passing compatibility test and two failures in `/tmp/plugin-spec-review-s12-alias-boundary.log`.

The final correction reuses the same resolver before command materialization. It retains frozen exclusions and adds current physical exclusions to the command view. An unresolved alias holds before execution; a valid directory alias with an absent leaf masks any retained entry at its physical location. No second resolver or ordinary Bash policy change was introduced. The local regression failed before correction in `/tmp/plugin-runner-alias-red.log`; all eleven credential controls passed afterward in `/tmp/plugin-runner-alias-credential.log`.

The independent reviewer approved both corrections after running all five unchanged local controls and all 28 production command-runner entries. Outputs: `/tmp/plugin-spec-review-alias-final-controls.log` and `/tmp/plugin-spec-review-alias-final-suite.log`. All 23 source hashes matched the final manifest. The original probe source, dependency files and original 1,255-line fixture/test prefix remained unchanged. The original probe source SHA-256 is `0844165dab5034ed7975e27159e5aa468600d4bad835ebeb2b7618207eec14b9`; the preservation manifest is `/var/tmp/demoncoder-hook-runner-checks/plugin-spec-review-original-k8rsmjga/manifest.json`.

Only synthetic local canaries were used in these controls. No real credential values were used or read by the probes.

## Process ownership and its limit

An implementation review found that supervisor exit alone does not prove that its sandbox descendants have finished. Releasing snapshot mounts, runner capacity or the mutation guard at that point could release resources while effects remain possible.

The correction uses a bounded status channel and a separate launch gate. The host opens a kernel process handle for sandbox PID1 and verifies its namespace and parent identity before authorizing package execution. The trusted inner wrapper starts with only the host baseline environment, requires the exact launch byte, closes the launch descriptor, and then executes literal argv with the declared package environment. EOF or a wrong byte cannot start package code.

After a launch token may have been sent, every return/drop path retains mounts, input, runner capacity and the mutation guard until the namespace process handle reports completed shutdown. Supervisor death revokes the lease and stops the pinned namespace before resources are released. Unobservable or stuck teardown remains held. Ordinary supervisor death must not permanently consume the workspace guard.

Actual detached-descendant tests cover normal exit, timeout, cancellation, owner hold and supervisor death. Each verifies that a follow-up write through the same runtime proceeds only after namespace shutdown, with no later heartbeat writes. An actual NativeSession owner-death test observes the supervisor and detached descendants stopping. Pre-token controls cover owner revocation and supervisor death after the namespace is pinned, with no package effects and bounded cleanup. The final broad suite includes these tests.

The earlier unadmitted path has a narrower guarantee: if the supervisor dies before trustworthy PID1 status can be validated, there is no process-handle proof of bootstrap teardown. No launch token means no package effects; the implementation releases after supervisor exit on that path. The parent accepted this distinction and the specification reviewer found no additional concrete defect. Normal pre-token cancellation still uses the supervisor's descendant reap. This exception must not be described as proof of full bootstrap teardown.

Sources inspected for this decision: [bubblewrap 0.11.1](https://github.com/containers/bubblewrap/blob/v0.11.1/bubblewrap.c) and [Linux process exit ordering](https://github.com/torvalds/linux/blob/master/kernel/exit.c). Installed bubblewrap controls also verified that an unconsumed inherited descriptor reaches the wrapper, the launch byte permits execution, EOF refuses execution, and the descriptor is closed before the payload.

## Executed checks

Final broad command, exit 0:

`TMPDIR=/var/tmp/demoncoder-hook-runner-checks CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test --all-targets`

The parent independently parsed `/tmp/plugin-runner-fd-all-targets.log`: **533 passed, zero failed, sixteen ignored across 36 targets**. All 30 command-runner entries passed: 27 behavior tests and three subprocess fixture entries. Clippy with `--all-targets -- -D warnings`, formatting, and whitespace checks also passed. Their retained logs are `/tmp/plugin-runner-fd-clippy-final.log`, `/tmp/plugin-runner-fd-fmt-check.log`, and `/tmp/plugin-runner-fd-diff-check.log`.

The sixteen ignored entries include separately driven installed-backend/LSP, live Oracle/public-assessment, inherited-descriptor and PTY/subprocess fixtures. They were compiled, not executed by this suite. Their exact names are retained in the manifest. No installed/live or complete-plugin evidence is claimed here.

An earlier bounded failure demonstration changed only the production snapshot fd4 binding from read-only to writable. The unchanged confinement test failed with exit 101 and `snapshot was writable`; exact restoration passed with exit 0. Evidence: `/tmp/plugin-runner-confinement-negative.log`, `/tmp/plugin-runner-confinement-restored.log`, and `/tmp/plugin-runner-confinement-probe.json`. This is historical evidence for that guard; the later credential corrections are verified separately above.

An earlier broad run with default test concurrency was intentionally interrupted and replaced by the bounded run. Its output is retained in `/tmp/plugin-runner-all-targets-interrupted.log`; it is not a passing check.

## Static findings and quality review

Ripwire remains nonzero. Final quality-delta exited 2 with 147 findings, including 43 gating findings. Test-gate exited 4 with all 31 targets and 231 untested-symbol rows, without truncation. Resolver and command edit checks reported zero incompatible callers. Raw artifacts are named in the final manifest; per-row dispositions are in `/tmp/plugin-runner-fd-static-dispositions.md`.

The dispositions distinguish actual complexity and size from trait/test reachability and resolver limitations. Independent quality review assessed them and found no remaining issue after the staging correction. The six added gating clone matches concern a test-only path accessor and unrelated helpers, not shared production behavior. These reports are not a clean static result or runtime coverage claim.

## Closed quality finding: staging descriptor use

The independent quality reviewer reproduced descriptor exhaustion in snapshot staging. Under an isolated process with a 1,024 soft descriptor limit, a 100-file workspace captures and launches successfully. A 600-file workspace also captures successfully, but materialization fails with `Too many open files` before command launch and holds the guarded write. SnapshotMount retains a parent and object descriptor for every entry until its reverse metadata pass; directories retain additional cleanup descriptors. This contradicts practical use of the admitted snapshot size, which permits up to 20,000 entries.

The local harness exited 101. Source: `/var/tmp/demoncoder-hook-runner-checks/demoncoder-quality-fd-t7rg9ilo/src/lib.rs`; output: `/tmp/plugin-quality-fd-regression.log`. No repository source changed. The higher-limit controls below confirmed attribution. The required correction was to bound live staging descriptors while preserving metadata restoration and cleanup, without reducing the snapshot contract.

The reviewer confirmed the same retained-descriptor problem for directory counts: 100 workspace directories pass while 600 exhaust staging descriptors; 100 package directories pass while 1,100 exhaust package staging descriptors. The unchanged flat-file and directory test binaries all pass when only the isolated descriptor limit rises to 2,048. This confirms attribution to descriptor retention rather than snapshot or import validity. `/tmp/plugin-quality-fd-evidence.json` (SHA-256 `fa3cf6d3ae9a3877a963a1471bd738b7e2bd8f92ee0fdf66750ca2c0d90972f0`) pins the harnesses and outputs. All 23 candidate source hashes remained unchanged. The reviewer released the source for one bounded correction covering both snapshot staging and package-directory cleanup.

## Bounded staging correction verification

The correction restores file and symlink metadata immediately, then restores directory metadata in postorder using the unchanged shared metadata verifier. A shared staging owner retains one root descriptor and relative path/device/inode records. It reopens directories with safe beneath-root resolution, verifies identity, and releases each temporary descriptor. Cleanup resets permissions parent-first. Identity rejection retains the temporary tree and prevents TempDir from deleting a replaced pathname automatically; fixed bounded warnings report cleanup failure without changing the original operation result. Existing depth and entry limits are unchanged.

The original reviewer resource harnesses now pass at the same isolated 1,024 descriptor limit. Local tests also pass with 1,200 entries in each category, verify no hook-staging directory remains, and cover restrictive modes, identity rejection, actual Drop behavior and partial metadata failure. All five prior specification controls pass. The final all-target suite reports 533 passed, zero failed and sixteen ignored across 36 targets; all thirty command-runner entries pass. Clippy, formatting and whitespace checks pass.

The parent independently verified all 24 source and 40 artifact hashes in `/tmp/plugin-command-runner-fd-manifest.json`, SHA-256 `f207d6f19d37218297deacc5281f9522d925064c6dbb8d3d43baa0ef5f057975`. Handoff: `/tmp/plugin-command-runner-fd-handoff.md`. Static results remain nonzero: 147 quality rows with 43 gating, and 31 test targets with 231 untested-symbol rows. Both independent reviews approved this frozen correction as recorded below.

Specification regression review approved the frozen staging correction with no findings. It independently passed nine runner unit controls, thirty command-runner entries including the isolated descriptor-limit matrix, and all five unchanged credential controls. Logs: `/tmp/plugin-spec-review-fd-unit.log`, `/tmp/plugin-spec-review-fd-integration.log`, `/tmp/plugin-spec-review-fd-existing-controls.log`. All 24 source hashes matched. Quality re-review subsequently approved the original resource correction and the new helper.

Independent quality re-review found no remaining Critical, Important or Minor findings. It reran all three original resource regressions at the unchanged isolated 1,024 descriptor limit, nine unit controls, and the larger 100/1,200-entry resource/cleanup integration. All passed, as did whitespace checking. All 24 source and 40 artifact hashes matched; the original reviewer evidence and ten historical artifacts remained unchanged. Exact independent results are in `/tmp/plugin-quality-fd-rereview-evidence.json`, SHA-256 `f7cf5edbf0a5fe1eae88cb9f569c81b2b002322493cf24fc8ea34c8d63f63df1`.

The parent's final audit found no unresolved issue in this bounded prerequisite. Changes reuse the existing admission, policy, metadata and receipt mechanisms; the new staging helper owns one resource-lifetime responsibility. Compatibility, failure behavior, retained outcomes, bounds and cleanup have executed controls and independent review. The ignored installed/live obligations and the early unadmitted bootstrap limit remain explicit. The complete commitment is still in progress.

## Final staged-tree checks

The parent reran Ripwire after staging. Quality-delta retained the identical 147 rows and 43 gating findings. Test-gate listed 32 targets and 250 untested-symbol rows: staging changed its discovered row set, without a source edit. Independent review classified all additions: 23 named tests have exact passes in the retained broad log, and five existing helpers are exercised by passing tests. The newly listed Rust targets passed (verification_workflow: five; workflow_store: sixteen); the omitted command-runner target still has thirty passing entries. Approval remained unchanged. Per-row evidence: `/tmp/plugin-quality-parent-static-dispositions.json`, SHA-256 `ce0620bc536b6f1de51951692d922197faaa16e576ad7ba3b7328cf88938102d`. Final raw reports: `/tmp/plugin-runner-parent-quality-delta.xml` and `/tmp/plugin-runner-parent-test-gate.xml`, exits 2 and 4 respectively.

The separate Python terminal hint is not covered by Rust test execution and needs an explicit requirement. The parent ran `python3 tests/verification_workflow.py --requirement VERIFY-002` and `--requirement VERIFY-005` against the built candidate using the private test TMPDIR and explicit DEMONCODER_TEST_BINARY. Both exited 0. Outputs: `/tmp/plugin-runner-parent-verify002.log` and `/tmp/plugin-runner-parent-verify005.log`. These establish the selected confinement/cancellation and cumulative-allocation terminal cases; other complete-commitment checks remain pending.
