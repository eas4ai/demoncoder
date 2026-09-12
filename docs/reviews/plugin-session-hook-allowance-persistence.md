# Explicit session-hook allowance persistence

Status: scoped implementation, focused and full parallel verification, and fresh
specification and quality reviews pass. This prerequisite does not
enable handler execution or complete lifecycle dispatch.

The current runtime has only task/delegation funding. SessionStart and SessionEnd
model and service runners remain unavailable until explicit session funding and
exact admission ownership exist. The selected first action introduces optional
public configuration and durable storage in the existing runtime, preserving
original task limits and cumulative clock behavior.

The implementation requires all three explicit invocation options together:
`--session-hook-seconds` (1–86400), `--session-hook-model-calls` (0–4096), and
`--session-hook-tool-calls` (0–4096). Omission returns no grant. CLI constraints and
the programmatic helper both reject partial or invalid limits. Task construction
still requires positive model and tool limits; only the separate session validator
accepts zero slots. Both use the existing Allocation clock/counter implementation.

The optional `session_hook_allowance` record field defaults to absent for older
records. The configured runtime open compares original optional limits before
recovery mutations or writes. Identical options retain the original allocation;
changed, removed or newly added grants fail with the restore-options/new-session
choice. Resume and guarded updates checkpoint both clocks. Task changes and
reconciliation do not replenish or transfer session funding. Main passes the
explicit grant before adapter startup; legacy open methods retain their signatures
and default to no grant.

Actual model/tool spending, service limits and runner execution require subsequent
production-path tests and are not established by this persistence action. Existing
lifetime runner denials remain unchanged. Seeded counters in persistence tests
prove storage behavior, not host admission or model use.

## Focused evidence

Raw logs, negative-control source patches and independent reviews are retained in
`/home/shawn/demoncoder-check-tmp/` with prefix
`session-hook-allowance-persistence-`. They are development checks, not Cairn
evidence receipts against a committed tree.

| Evidence | Actual result |
|---|---|
| `red-source.patch`, `red.log` | Initial tests fail on absent CLI options and the absent durable field, exit 101. |
| `resume-write-control-source.patch`, matching log | Moving the comparison after the recovery write fails the unchanged-state-byte assertion, exit 101. |
| `resume-reset-control-source.patch`, matching log | Recreating the grant on identical-options resume fails the original start-time assertion, exit 101. |
| `checkpoint-control-source.patch`, matching log | Removing the guarded checkpoint loses persisted clock rollback invalidity, exit 101. |
| `focused-green.log` | Nine integration tests pass on restored source. They exercise option validation, actual disk resume, task separation, expiry and rollback, and main's startup ordering. |
| `checkpoint-green.log` | The guarded-update clock test passes, retaining consumed funding and completed evidence while recording clock invalidity. |
| `focused-collision-green.log` | All ten integration tests pass after the directory repair, including 32 real concurrent runtime opens and reopened grants. |
| `unique-directory-fixed.log` | All five store tests pass, including the preservation, error, bound and confinement cases described below. |
| `spec-review.md` | Fresh independent specification review passes for the bounded changes and records eight source hashes. |
| `quality-review.md` | Fresh independent quality review passes after checking final execution evidence, all source hashes, static findings and all 14 production rules. |
| `full-regression-repaired.log` | Default-parallel `cargo test --all-targets` exits 0: 45 suites, 894 passed, 0 failed, 17 ignored; 564.87 seconds summed suite time. |
| `pty-output-limits.log` | The separate application/terminal suite exits 0: 3 tests in 4.076 seconds against the final built binary. |

The actual-main test intentionally reaches an unavailable adapter credential
after durable creation, then checks that mismatched resume options fail earlier.
It makes no live model request. Controls compile and fail on the required behavior;
their retained source patches identify the violated protection. The final source
restores all three protections.

The parent independently parsed the full regression: 92,885 bytes, SHA-256
`8755e500832968be204d3ccc160e2790d8cb41cd2492c037ce6460057d774a03`.
The raw log retains its command and exit 0. The repaired candidate also passes
all-target Clippy with warnings denied, formatting, and the parent's
`git diff --check`. All eight source hashes still match the reviewed candidate.
The terminal-tested binary SHA-256 is
`6722d5e480f2e9fbee1a7732bda7a0230d513f7fb86c7fecfc4265e07e256afd`.

Two private hook staging cleanup warnings remain in the full log. The unchanged
`automatic_drop_preserves_replacements_after_identity_rejection` test deliberately
replaces a child or root in two cases. Drop refuses cleanup after the identity
change, emits that warning, and the passing test checks preservation. No cleanup
behavior or warning suppression changed.

## Parallel session creation finding

The first default-parallel full regression passed 343 of 344 library tests; an
existing provider-failure test could not create its session directory because the
timestamp-plus-process-ID name already existed. A separate scratch probe linked
to the unchanged current library made 32 synchronized legitimate runtime opens:
two succeeded and 30 failed with the same collision. Its source and output are
retained in `session-hook-allowance-persistence-collision.rs` and
`session-hook-allowance-persistence-collision-control.log` in the scratch evidence
directory. This is a real runtime defect, not a reason to lower test concurrency.

The [recorded correction](../decisions/reserve-unique-session-directories-atomically-under-concurrent-startup.md)
uses bounded unique candidates and atomic directory reservation under a pinned
parent descriptor. Only directory-creation AlreadyExists may retry; subsequent
initialization failures must propagate. Existing candidates remain untouched.
The repair validates a nonempty single-component prefix and tries suffixes 0–127
under one pinned, validated private parent descriptor. Only `mkdirat` returning
AlreadyExists advances the loop. Once a candidate is reserved, the shared original
initialization runs exactly once, retaining no-follow opens, private modes,
exclusive locks, validation and durability synchronization. Exact-name
`Store::create` keeps its signature and collision behavior.

The five store tests check unchanged existing record bytes, a skipped symlink and
outside canary, private modes and exclusive ownership, rejected prefixes without
outside effects, exhaustion after 128 occupied candidates, a real lock-file
AlreadyExists error after one initialization, a different mkdir error, and writes
staying under the pinned parent after its pathname is replaced by an outside
symlink. The first run exposed fixture directories with mode 755; the fixtures
were made private with the existing helper. Production validation was not relaxed.

`concurrent-red-source.patch` and `concurrent-red.log` retain the new real-runtime
test failing against the old creator with 27 rejected opens, exit 101. The corrected
integration test admits all 32 opens into distinct, exclusively owned and reopenable
directories. The original standalone probe also now admits all 32 with zero rejected
opens (`collision-fixed.log`). The repaired default-parallel full regression now
passes; the earlier 343/344 failure remains recorded.

## Static findings and review limits

The repaired candidate's qualified edit checks pass. Ripwire `quality-delta`
returns 2, with 42 rows and three gating rows; it is not a passing check. The
quality reviewer independently examined the findings. Two gating rows report the
same 25-token connection-fixture literal shared with an existing private test
helper. A public production helper would couple separate test compilation units
without sharing an algorithm. The third gating row is main's necessary explicit
configuration wiring and accumulated churn. No suppression or baseline was added.

The remaining rows concern executed test entry points and trait/type reachability,
the existing open body moved behind the compatible wrapper, test matrices and
fixture size, and historical churn. The moved open method remains cohesive around
one locked durable open/resume operation; new branches validate the optional grant
and checkpoint its clock. One tests-module size row has no corresponding source
diff and reflects a repeated symbol name in the structural index. No required
revision was identified in the source review.

Ripwire `test-gate` returns 4 and does not execute tests. It reports seven changed
symbols, 962 impacted symbols, 41 mapped test files and 420 unmapped symbols; only
the first 25 unmapped rows are displayed. These are broad coverage hints across
adapters, lifecycle dispatch, terminal flow, language services, storage and task
operations. The full Rust suite and separate terminal script supply executed
evidence; they do not establish coverage of every unmapped symbol. Ignored
live-provider and installed-backend qualification remains separate work in the
same commitment and is not claimed by this prerequisite.

## Production self-audit

1. Configuration, main startup, durable open/resume, task updates and store creation were traced.
2. The change adds the optional grant and fixes the concrete collision exposed by regression.
3. Existing allocation and store initialization algorithms are shared; no second ledger or store is created.
4. Legacy fields and open APIs remain compatible; task limits and exact-name store creation retain their contracts.
5. Resume mismatches are actionable; original initialization errors propagate and no secrets enter reports.
6. Atomic reservation preserves private descriptors, modes, locks and no-follow traversal; hostile prefixes and symlinks are tested.
7. Exact-option resume preserves funding, time and uncertainty; failed option changes cannot rewrite recovery state.
8. Creation retries are bounded; concurrent opens and clock rollback have actual behavioral controls.
9. Lifecycle dispatch remains the sole active plan item; this prerequisite is marked verified after code, checks and both reviews pass.
10. Focused, default-parallel Rust, terminal, formatting and lint checks pass with retained evidence.
11. Failed controls, the original parallel failure, nonzero static checks and absent runner spending are disclosed.
12. The collision was investigated and repaired within the existing runtime, with the failed parallel check retained.
13. Both independent reviews and the final checks pass; no required revision remains in this bounded prerequisite.
14. Options, errors, decisions and reports name the developer's concrete funding and resume choices.
