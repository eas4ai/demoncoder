# Provider and agent Settings review

commitment: provider-agent-settings
commit: 8168683dca1b996460e810dfa8a82e96b17b06d5
examined:
  - SET-001 through SET-008: shared editor, provider checks, private store, live terminal, role admission, recovery, documentation and installed release.
  - CONN-003, CONN-004, CONN-005 and START-003: authentication, loop ownership, refusal and private startup.
  - VERIFY-003, VERIFY-005, VERIFY-006, SUB-005, SUB-007, ORCH-006 and ORCH-007: verification, integration, allocation, cancellation and recovery.
  - REM-002, REM-004, LEARN-006 and LEARN-007: truthful identity/evidence display and unchanged lesson authority.
findings:
  - resolved: SET-007: Optional provider placeholders now respect the connection limit; boundary saves preserve every existing connection and reopen successfully.
  - resolved: SET-008: Corrected the comma-separated Host paths declaration; specification lint passes.
Status: complete

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

The new boundary test reproduced the rejection after serialization and reload.
The corrected editor adds optional choices only while capacity remains. The same
test now passes for 61 through 64 saved entries and checks that every original
connection is unchanged. The manual names this limit and how to add a provider
when the configured list is full.

## Todo

- Complete: checked provider discovery and shared onboarding selectors, with
  nine probe tests and 13 production provider cases passing.
- Complete: live assignment, persistence and role admission, with four store
  tests, six live-terminal cases and six role/request/recovery cases passing.
- Complete: installed release demonstration and first inherited evidence sweep.
- Complete: refreshed committed evidence after the reviewed connection-limit
  correction and completed the final review with no open in-scope finding.

## Final assembled review

The current candidate has fresh passing evidence for every selected requirement.
The last Settings mechanism ran 14 settings unit tests, 13 provider PTY cases,
six live Settings cases and six role/recovery cases. It repeated all 25 PTY cases
after installing the release. The installed executable and target/release binary
have identical SHA-256:
`dfb3f79dc561bfb8e6c7042a2193c069218c31b5f48b2f5b3aa9701d8073b9a3`.
The installed executable reports demoncoder 0.1.2. The full inherited connection,
startup, verification, subagent, orchestration, status and learning mechanisms
were rerun after the capacity correction and passed the selected requirements.

The review traced editor cancellation, catalog replacement, masked key entry,
role selection, final save and reopening together. Empty or failed catalogs cannot
produce a selected model. Returning from a model submenu preserves its previous
assignment. Saving validates selected catalogs; deselected assignments stay
unresolved. Bounded rows and explicit capacity preserve usable saved state. The
capacity defect found by this review is now demonstrated failing and corrected.

The persistence review followed the same parsed bytes into revision comparison,
private locking, atomic replacement and runtime publication. Conflicting or failed
saves do not activate a draft. Publication uncertainty is read under the same
state lock and blocks new work until repaired. A post-rename directory-sync failure
was reviewed in source; the mechanism does not claim to simulate every filesystem
or hardware failure. Existing credential-file protection is retained even when
launching without a saved API key. Provider errors and catalogs cannot expose the
synthetic credentials used by the negative cases.

The admission review followed Creator capture, active-task pinning, queued Worker
restoration, late Reviewer selection and independent Oracle/Advisor/Judge requests.
Default-role recovery exceptions require the same launch enablement and preserve
checks, access and limits. Original operation identities and allocations survive
new defaults. Opaque backend changes begin distinct contexts; the installed tests
verify the notice and actual process/request boundary. Assigned models do not
enable work or give tool authority to decision roles.

The static warnings were considered with the source rather than counted as test
passes. The expanded delegation comparison explicitly covers each enabled default
role while comparing all other saved authority. The keyboard dispatcher branches
over its four page states and their controls. Small clone matches are cancellation
and event-copy idioms; public compatibility wrappers and trait callbacks remain
intentional. No additional correctness or maintainability defect was established
from those warnings. No baseline or acknowledgement was added to hide them.

The updated manual matches the checked provider sequence, inherited and explicit
assignments, Save/Enter/Escape controls, live timing and the capacity limit. An
unrelated older fixed-output-limit paragraph was captured with `cairn backlog`
as `correct-the-older-output-limit-paragraph-in-the-connections-manual`; it predates
this workflow and remains outside this commitment. No reference code was copied,
and the original pi reference is not represented as the unavailable oh-my-pi tree.

### Verification limits

Controlled local API and CLI peers establish routing, failure handling and
application behavior. They do not promise that a commercial account can use every
advertised model, or that a later backend release keeps the same protocol.
Unselected historical live-provider records remain stale and are not represented
as refreshed paid-provider evidence. The application makes the authentication
observation and model-availability limits visible.

### Production self-audit

The review checked the production rules against this commitment: scope and
existing behavior, cohesive implementation, boundary validation, secret handling,
private persistence and recovery, bounded background work, meaningful failing
and corrected tests, honest evidence, documentation and plain-language choices.
The implementation retains existing interfaces where required, adds no new
execution authority or dependency, and records the known limits above. All work
and verification items are complete. I am satisfied with the scoped change; no
open in-scope finding remains. This final review changed no runtime code.
