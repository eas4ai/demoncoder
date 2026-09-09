# Audit remediation review

commitment: audit-remediation
commit: a4557b70045e03cea81a044602582207445b0f03
examined:
  - CODE-007: final tool boundary, installed backend fixture, shared executor and mechanism runner.
findings:
  - resolved: AUD-006: The installed Codex fixture and cumulative gate now pass against the supported route.
  - open: AUD-001: Environment-declared credential roots can enter snapshots and reviewer requests; cover the shared private roots and historical snapshot boundary.
  - open: AUD-002: The same missing private-root admission can export credentials through delegated capture and Git snapshots.
Status: in progress

## CODE-007 mechanism review

Read the current requirement/falsifier, coding-session declaration and runner,
the existing mechanism review, final executor admission, installed backend driver,
and Codex's current effective-route validation. The mechanism still checks real
effects and original results; its installed Codex positive case now has an invalid
configuration. The end-to-end audit ran that exact fixture and its result-cycle
variant on unchanged production inputs: both failed before a model request with
the explicit custom-provider rejection. OpenAI, Anthropic and Claude cases passed.
The deliberate denied direct-file and allowed Git-canary demonstrations in the
audit additionally show the cross-feature export defect addressed by AUD-001/002.

The corrected Codex positive demonstration is not yet possible with this fixture.
Do not treat the expected route rejection as a successful tool-routing proof. The
declared src/tests footprint includes the implementation and fixture dependency;
record the mismatch now and repair it as a separate implementation action. The
root commitment review remains open until corrected cases and cumulative checks
run against committed inputs.

## Protected export mechanism declaration

The audit-private-exports mechanism runs production workspace capture and review,
real Git worktree preparation and integration, the normal developer-access boundary,
and an installed terminal review using synthetic private canaries. Its footprint
includes shared Rust and Python dependencies and the specification. Installation
precedes the terminal case so a stale installed executable cannot pass this check.

The original implementation failed the new capture regression at the assertion that
retained contents exclude PRIVATE_RUNTIME_CANARY. The recorded pre-fix run had zero
passes and one failure; the same regression passed after the export-policy repair.
The Git case computes canary object IDs without writing them, then requires that
the objects do not exist after preparation or integration. This rules out retrieval
through parent Git tools; the developer-access suite independently tests denied
direct private-file reads. Public owned integration and private parent preservation
are checked too. Older snapshots are injected with private text and must not export
it. The installed case inspects the actual reviewer request and retained state,
requires public source and explicit exclusions, and completes verified acceptance.

This is a mechanism review, not the final independent commitment review.

## Generated-output mechanism

The initial committed AUD-003 check failed because the application rejected
--generated-output. The implementation adds explicit scope rather than increasing
the capture limits or weakening input freshness. Production capture tests require
an undeclared 9 MiB artifact to fail and a declared artifact to preserve source
identity. A real source edit and a changed declaration must produce new identities;
review across different declarations must refuse.

The terminal check now builds and rewrites 9 MiB outputs twice, verifies and accepts
the actual source, resumes with the same declaration, rejects a different declaration,
and refuses a check that changes an undeclared input. A real delegated child creates
a large output, passes validation and integrates its owned source while preserving
the parent's generated files. Git worktree tests separately prove that parent and
child generated canaries never enter retained Git objects or source deltas.

An independent read-only call trace confirmed that parent resume, reconciliation,
verification, review and acceptance all use WorkflowSession::snapshot, and that
child inspection, reconciliation and integration use the retained baseline scope.
Learning instruction reads and tool permissions are intentionally unchanged.

## Bounded review mechanism

The initial AUD-004 mechanism failed when the application rejected --review-context.
The corrected terminal cases retain a 1.2 MB public source baseline for verification,
then inspect the actual reviewer request for complete old/new changed source,
selected support, omitted-source identities and explicit not-reviewed labels.
Both selected context and changes-only review pass. A subsequent task changes the
large unselected file: review must refuse at the evidence limit before any request,
and acceptance remains blocked. Changing the context selection also refuses resume.

The formatter tests additionally require changed binary refusal, missing context
refusal, scope identity changes, and refusal when omitted identities alone exceed
the request budget. Generated-output and prior default-scope tests remain active.
The child terminal case retains the large source in its actual worktree, reviews
only complete changes and selected support, then validates and integrates source.
This demonstrates scope propagation without weakening capture or tool access.

The complete parent/child terminal fixture passed. The first child fixture used
four tool calls and exceeded its inherited 20-second test wait; a diagnostic run
completed normally in 23.31 seconds. This bounded-review case now makes one write
before validation and integration; four-tool coverage remains in the existing
assignment suite. Production deadlines and review requirements are unchanged.

## Anthropic append mechanism

The committed baseline fails the real accumulator's allocation-reuse test on its
first fragment despite 128 KiB of reserved capacity. The corrected helper appends
to the existing String after a checked byte-limit calculation. The same test now
keeps the pointer across 6,000 fragments for each text, thinking and signature
field, including combining Unicode, multibyte text, empty fragments and newlines.
It verifies exact complete contents after accumulation. Separate boundary tests
accept exactly 4 MiB, refuse the next multibyte fragment without changing contents,
and leave a missing field absent when its first fragment is oversized.

This is a deterministic storage-reuse observation, not a wall-clock performance
claim. Existing stream cancellation, UTF-8 transport and incomplete-tool-response
checks remain in the mechanism. The small internal change preserves field and
call contracts; no new public configuration or runtime owner is introduced.

## Cumulative gate mechanism

The committed AUD-006 baseline failed because no completed-product script existed.
The new gate names all 21 existing completed-product drivers, runs Cargo's full
all-target checks, and repeats installed backend tools/results plus audit and
verification/recovery terminal cases against the installed release. The ordinary
coding and connection drivers still validate paid live evidence by default. Their
explicit local-only option reports CODE-010 and CONN-001 as unverified, never pass.

Safe failure demonstrations used disposable copies of the shell drivers and
substituted command peers, not real providers: injected installed tool failure and
installed result failure each stopped the aggregate with exit 73 and no AUD-006
pass. Injected final live-validation failure still stopped each default runner;
local-only returned successfully with its explicit unverified marker. Unknown
options returned usage errors. These probes establish runner selection and failure
propagation only; the real gate remains responsible for product evidence.

The real cumulative gate completed successfully: all 21 drivers, full Rust tests,
formatting and Clippy, then installed-release tool/result cycles, private review,
generated outputs, bounded parent/child review, and all six verification/recovery
terminal cases. Paid live-provider and Oracle cases were explicitly unverified.

## Final parent examination

Reviewed the complete 00facef^..a4557b70045e03cea81a044602582207445b0f03
implementation, tests, runners, declarations, decisions and documentation without
changing code. Attacked private path selection, configured aliases, hard links,
old snapshot formatting, Git-object export, source/scope identity, child scope
propagation, mandatory changed-file evidence, request-size refusal, and subscription
route integrity. Capture still refuses multiply linked files before reading them,
pins regular inodes, and reads symlink text without following its target. Source
and review scope are validated at admission and resume and retained through child
preparation, inspection, verification, delta construction and integration.

The append path validates its complete byte count before mutation and appends only
the new fragment. The settings guard retains writer exclusion while alive and
explicitly unlocks its open file description when the transaction ends. The Codex
fixture serves only its loopback CONNECT peer and accepts only chatgpt.com:443;
it does not rewrite effective configuration or use real provider credentials.

All selected requirements have fresh passing receipts. AUD-006 receipt
20260909T001808764Z records exit 0 and the complete cumulative output. Examining
that retained output confirmed 21 unique completed drivers, both development and
installed CODE-007/008 passes, and no false CODE-010/CONN-001 paid-live pass. The
installed binary and target/release/demoncoder both hash to
0ac06942962c5b537df4b82a7dd20da337adc314e11b295c6cda56e67f3ddeaf;
the installed version command reports 0.1.2.

Ripwire's test gate named 20 test-file obligations; normal Rust cases and the
required PTY drivers ran, with the paid live Oracle explicitly separate. Its
static graph cannot prove dynamically driven coverage. A quality-delta comparison
used disposable source archives with the pre-remediation baseline pinned before
loading the final source. It exited 2 with 11 major metric suggestions, not a clean
scan. Examined the added capture branches, review formatter length, CLI fields,
compatibility wrappers and fixture setup. They implement the declared boundaries
in their existing owners; splitting them only to reduce the metric would obscure
the single capture/review checks. The dead-code suggestions include executed Rust
tests, serde callbacks and test-only wrappers. No unresolved behavior defect was
identified by this parent examination.

Self-audit against production rules 1–14: scope and ownership are explicit; changes
are confined to the agreed requirements and an inherited failing Settings lock;
validation, failure behavior, private-data boundaries and recovery identity remain
covered. Existing interfaces retain default wrappers; new scopes require explicit
launch choices. Tests include failing baselines and corrected production paths,
and documentation states limits and unverified paid availability. No speculative
feature, hidden permission expansion, or new runtime owner was added. The final
independent reviewer result is recorded below before acceptance.

## Independent review finding: relocated private roots

The independent reviewer reproduced an actual terminal failure on the committed
candidate using synthetic credentials only. With CODEX_HOME set to the workspace's
runtime-secrets directory, /task, /verify and /review completed, and the auth.json
canary appeared in both state.json and the actual reviewer request. DeveloperAccess
protects CODEX_HOME, CLAUDE_CONFIG_DIR and AWS_SHARED_CREDENTIALS_FILE, but snapshot
admission checks only explicit connection credential paths. Relative path filtering
also cannot recognize a workspace selected inside a private root. Delegated capture
uses the same incomplete boundary and must be repaired with the parent path.

The correction must check lexical and resolved private roots in both directions
before capture, preserving existing reserved-name exclusions and public projects.
Historical snapshots can also contain arbitrary private filenames from an earlier
environment. Because they retain no dynamic-root provenance, new review must refuse
those unsafe older versions and ask for a new task baseline. Historical records and
Git objects remain untouched. This finding is recorded before code changes.

The final independent verdict is fix-first. A delegated probe also confirmed that
runtime-secrets/auth.json entered the child and the shared Git blob database.
The reviewer found no other changed-scope defect after inspecting AUD-003 through
AUD-006, recovery, integration, settings locking and route validation. Its probes
used disposable projects under the designated scratch directory. Actual model,
effort and usage telemetry were unavailable. Paid provider availability remains
separate from the passing local installed-backend tests.
