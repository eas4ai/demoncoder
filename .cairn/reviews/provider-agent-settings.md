# Provider and agent Settings review

commitment: provider-agent-settings
commit: a864e430b5ead03df6649aff9bdfeef6c220d40d
examined:
  - START-003 startup mechanism against the current requirement and falsifier.
findings:
  - open: SET-007: Auto-added provider placeholders can exceed the editor connection limit and make the next Settings open fail.
  - resolved: SET-008: Corrected the comma-separated Host paths declaration; specification lint passes.
Status: incomplete

## START-003 mechanism review

Read the startup declaration, requirement, startup settings_lock/save paths,
production startup driver and its onboarding helper. The declared src/tests
inputs cover the runtime and imported fixture helpers. The script runs the real
binary and derives per-requirement results from unittest assertions.

Ran `python3 tests/startup.py`: six tests passed across START-001 through
START-003. The safe negative cases use disposable directories: a symlinked home
settings directory and an explicitly configured shared parent both refuse startup
without changing their target/parent. The corrected case secures an owned 0775
default directory to 0700, preserves a canary, saves private settings and reaches
the session. Existing connections survive repair and new-workspace trust.
Foreign ownership is checked in source before permission repair; creating a
foreign-owned directory is not available to this unprivileged test process, so
this branch was inspected rather than demonstrated by changing host ownership.

No START-003 behavior mismatch found. The old receipt needs regeneration for
current Cairn execution-order and output-digest metadata. The UI tests currently
expect the old setup prompts and must be updated when SET changes that flow,
while retaining these filesystem and session assertions. The SET specification
has a separate formatting defect recorded above; this review changes no code.

## Connection baseline runner repair

The first baseline stopped on stale CONN-001 live records before executing the
selected authentication and ownership checks. Moved that independent historical
validator to the end of the runner. It still fails on stale evidence, and its
acceptance rules and records are unchanged. Earlier checks can now emit their
own results. No live provider requests are made by the historical validator.

The development run now reaches and passes CONN-002 through CONN-006, including
the selected authentication, ownership and capability checks. It exits one at
the unchanged historical live-record validator, which also requires committed
specification inputs. The subsequent Cairn run records the committed result.

## Initial behavioral failure demonstration

`python3 tests/provider_agent_settings.py` against the unchanged debug binary
failed its production PTY checkpoint after project trust: `Space toggles` never
appeared. The process remained in the old numeric provider prompt. Exit 1,
one test failed in 8.045s. This is the checkbox falsifier, distinct from the
earlier missing-runner unverified receipt. Authentication and live assignment
proofs remain pending; this test alone cannot establish SET-001.

## Implementation verification before committed evidence

The development binary now passes 13 provider PTY cases, six live Settings cases
and six actual role/request/recovery cases. Nine probe and four persistence tests
pass. The full `cargo test --locked` run passed 230 tests with six ignored;
`cargo clippy --all-targets -- -D warnings` and formatting check pass. Existing
onboarding, startup, configuration and four-adapter terminal cases pass. These
development runs are not Cairn evidence receipts.

The new mechanism runs these assertion-bearing cases before emitting requirement
markers. It installs the release and repeats all three production-terminal suites
before emitting SET-008. Authentication failures, missing logins, unavailable and
malformed catalogs, timeout/cancellation, reflected synthetic secrets, raw-file
conflicts and a held private-file lock are negative cases, paired with successful
setup/save/request cases. No check invents a pass for a missing installed binary.

A held `/correct` case reproduced the old Reviewer timing failure after the
saved default changed. The current binary passes by resolving Reviewer when
review begins. Queued workers still issue worker-old requests after the default
becomes worker-new; newly admitted children issue worker-new. Restart retains
queued identity and model/tool counts without replay. The same cases inspect
original task Creator identity and distinct tool-free Reviewer/Advisor/Judge
requests. The Oracle case performs one disposable outside-workspace write only
after a tool-free request to its explicitly assigned model.

Independent source review found three runtime issues during implementation:
publication uncertainty was read before acquiring its state lock, correction
resolved Reviewer too early, and recovery treated a not-yet-invoked default
Reviewer like an immutable explicit assignment. All were corrected. Follow-up
source review confirms those fixes and found no additional concrete regression
in queued Worker restoration or operation attribution. A separate source review
examined bounded probe/process cleanup and reported no concrete findings.

Ripwire was run as requested. The unfiltered crawl included ignored reference
checkouts, so the useful rerun excludes reference and target. Quality delta exits
2 with 67 major entries across code and tests, including historical churn,
expanded dispatch/constructor complexity and small clone matches. Its dead-code
rows include dynamic/trait dispatch and retained public compatibility wrappers;
its `prepare` complexity locator conflates same-named functions. Test-gate exits
4 with 49 harness obligations and 111 symbols it cannot map to tests; Python PTY
and script-to-binary execution are not fully modeled. This is not a passing static
gate. The application paths are covered by the compiled Rust suite and selected
production mechanisms; inherited mechanisms and the final assembled review remain
pending. `bind_creator` edit-check reports one caller and no incompatible arity.

## CONN-003 fixture repair

The first inherited connection run correctly refused the old authentication
fixture's incomplete Codex account response. Added the required
`requiresOpenaiAuth` observation to that fixture; the runtime check is unchanged.
The full development connection runner now reports passes for CONN-002 through
CONN-006, including rejected/missing/expired credentials and wrong billing routes.
It still refuses stale historical live-provider evidence for unselected CONN-001.

## Final review finding: connection-limit stability

At 64 existing named connections, Editor::new accepts the configuration and then
adds placeholders for missing built-in adapters. Saving can therefore retain more
than 64 entries; the next Settings open rejects the file it just saved. The review
found this by tracing new/apply/persistence together, beyond the ordinary PTY
catalog sizes. Keep existing connections and enforce the same capacity while
adding optional placeholders; verify reopen stability at the boundary. No code
was changed during this review.
