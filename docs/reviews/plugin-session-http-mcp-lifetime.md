# Native session HTTP and MCP lifetime integration

Status: implementation, final-source checks and independent specification and quality reviews pass on freeze4. This is a bounded lifecycle prerequisite; the full
skills/plugins/hooks commitment and its lifecycle-dispatch item remain open.

## Behavior

Actual native SessionStart and SessionEnd can execute synchronous HTTP and MCP
hooks from their original explicit SessionHooks grant. Preparation, admission,
queued waits, effects and delivery retain the exact original budget reference and
live occurrence. Task funding cannot fill an omitted or expired session grant.
Transport calls keep existing time and service-local limits without spending
model, backend or Agent inspection-tool slots. Command-only observation remains
grant-free, and existing task/model accounting remains intact.

A managed MCP connection retains its original task or actual native host lifetime,
cumulative deadline and call count. Successful startup can release observation
leases while the connection stays idle. Every later invocation still requires its
own exact pending reservation, matching service identity, configured role,
credentials and retained view. A changed stdio snapshot requires readmission; it
cannot refresh or revive the existing handle. Task stop, acceptance, replacement,
archive or unrelated cancellation cannot replenish or transfer session ownership.

Live native authority is memory-only and omitted from serialized records. The
NativeSession operation's complete flag records a fact; it does not end the host.
The actual session-run guard revokes on abort/drop even when other code retains
EventSink, runtime or service handles. Normal end dispatch precedes revocation
and owned cleanup. Startup keeps 27 seconds for observation and three for cleanup;
end keeps two plus three seconds within the application's eight-second reserve.
Ordinary session close still runs after earlier failures.

Cancellation during a later startup handler stops services started by earlier
handlers before readiness. It preserves the original host and grant for separately
authorized end-only work when no hold remains. An uncertain hook and unfinished
lifecycle receipt must be reconciled through the existing process before that
admission; clearing a flag or reviving a cancelled service is not reconciliation.

## Findings and corrections

Compiled controls demonstrated missing native HTTP/MCP execution, accepted late
HTTP and MCP replies, discovery-to-call after exact hook settlement, changed reads
still allowing tools/call, post-persistence fallback to an expired unrelated task
clock, and an unauthorized protocol-generated SSE ping response. The correction
checks exact invocation and retained-view authority at protocol sends and delivery,
including after durable service admission. HTTP redirects remain disabled.

The post-persistence test reads the saved state and confirms the original-budget
PluginService Pending record before its one-shot fault changes authority. Denial
then produces zero initialize traffic while retaining pending/uncertain work.
This proves the durable boundary, rather than merely changing an in-memory record
before persistence. Original result/usage settlement remains separate from fresh
execution permission.

The compiled stop control observed only seven of eight service permits available
when stop returned with a retained handle. It proved incomplete release at return,
not a permanent leak by itself. The monitor now observes revocation independently
of a busy connection lock, closes without waiting on itself, and releases ownership
before signalling completion. Explicit stop joins that ownership. Confined stdio
cleanup retains processes, descendants, mounts and permits until actual teardown.

The partial-startup control found an earlier idle service surviving cancellation
of a later handler. Corrected controls observe process absence before readiness
and join all fixtures before assertions. The controlled pending Command callback
leaves an unsettled receipt, rather than setting recovery_pending. End-only work
correctly sends zero requests before explicit reconciliation and one afterward,
without resetting the host or grant. The peer and its detached descendants are
actual confined processes; the pending callback is a controlled fixture.

Fresh QUALITY review found a further idle stdio boundary: clearing the completed
startup invocation also removed the view check before protocol replies. The
correction retains an independent TransportView for service maintenance and checks
it immediately before every write. Active writes additionally retain the current
invocation's full view check and original owner/deadline validation. No completed
hook is made executable again, and idle polls do not repeatedly scan full snapshots.

The final compiled `red-idle-stdio3` control passes ordinary and equivalent-policy
idle pings, but fails both changed-read and changed-protected-target cases because
they receive forbidden replies. Its mutant removes only the new service-view
check and preserves active invocation checks. `green-idle-stdio2` passes all four
cases, including observed descendant teardown. Changing an alias between two
already-protected targets preserves the effective policy and remains valid.

Several historical failures were fixture or diagnostic issues, not authority
proof. Early compiler errors and a role/generation mismatch remain separate. The
first idle fixture also signalled its confinement wrapper; the corrected fixture
selects the Python executable. An initial credential assertion wrongly expected
an equivalent-policy alias remap to revoke authority. An earlier broad-liveness
assertion ran after refusal correctly introduced uncertainty, and another checked
recovery_pending instead of the actual unfinished receipt. Raw failed runs remain
retained, including those named green. The initial exact MCP loop proved the
tools/list boundary; a separate `red-mcp-reply` proved its late tools/call response.

Two unchanged assertions caught diagnostic wording regressions. Their common
original prefix was restored without weakening either assertion. Clippy caught
a nonminimal boolean in the test-only persistence probe; it was simplified without
a suppression. These failures remain visible alongside corrected results.

## Executed verification

Raw evidence is retained under
`/home/shawn/demoncoder-check-tmp/session-http-mcp-`.

| Check | Result |
| --- | --- |
| Final `cargo test --all-targets` (`full-final3`) | Exit 0; 964 passed, zero failed, 17 ignored across 45 suites; 570.98 summed suite seconds. |
| `cargo clippy --all-targets -- -D warnings` (`clippy-final2`) | Exit 0. |
| `cargo fmt -- --check` (`fmt-final2`) | Exit 0. |
| Final runtime controls (`runtime-final`) | Exit 0; 12 passed. |
| Final complete transport targets (`transports-final2`) | Exit 0; 32 HTTP and 39 MCP passed. |
| Separate final-binary output-limit PTY | Exit 0; three passed in 4.102 seconds; executable unchanged before/after. |
| Fresh SPEC re-review | PASS on freeze4; the idle branch omitted by the first review is corrected and independently re-reviewed. |
| Fresh QUALITY review | APPROVED on freeze4 and final evidence; idle-write and lint findings resolved. |
| Final Ripwire edit check | Exit 0 for TransportView, with incomplete graph coverage. |
| Final Ripwire quality delta | Exit 2; 130 rows, 54 gating. Not a pass. |
| Final Ripwire test gate | Exit 4; 46 requested paths and 472 unmapped impacted symbols. Not a pass. |

The parent independently checked final raw counts/exits, all 349 source input
hashes, the main executable and restored Cargo configuration. The consolidated
manifest binds 325 evidence artifacts and 46 compiled executables. The separate PTY
run uses the exact final executable directly and records its hash before/after.
Earlier manifest3 passed 960 tests with 17 ignored, but it predates the idle-write
correction. Earlier focused source variants and their limits remain documented
in the independent reports. Two broad-suite staging warnings come from unchanged
deliberate root/child replacement controls: cleanup refuses the substituted
identity, the test verifies preserved content, then explicitly removes it.

Static dispositions cover actual Rust trait/test uses missed by the graph,
required ownership branches, fixture matrices, setup duplication and churn.
The final refresh includes TransportView and all four idle controls. Neither
nonzero tool result is a passing gate or complete coverage. No acknowledgement,
suppression, test exclusion or metric-only rewrite was introduced.

## Limits and evidence identity

The retained-handle churn test proves release of the new native cancellation
registry and eight shared runtime service permits across 13 retained stopped
handles, using a fresh manager each time. The pre-existing same-manager admission
map still caps retained entries at eight. This does not block required reuse of
one configured startup/end handle or the separately admitted end-only fixture;
it is not proof of arbitrary public readmission.

Controlled fixtures and injected plans exercise production native paths. They do
not qualify actual external-backend lifecycle sources or public activation.
Codex SessionEnd MCP remains accepted-but-skipped by its frozen source contract.
Public management, changed-snapshot readmission, asynchronous session commands,
other lifecycle effects and complete package conformance remain required work.
Existing installed/live-source evidence retains its original candidate.

Timing is cooperative: an outer timeout cannot preempt arbitrary synchronous
filesystem or kernel reap operations. Ownership remains retained under that
condition. Observed process teardown is not an absolute OS scheduling guarantee,
and bounded HTTP close does not undo remote effects. The final run showed no recurrence of the earlier unexplained SIGILL; its cause remains unconfirmed.
The restored Cargo configuration is unchanged; no managed-backend rebuild was
needed. Current evidence capture uses explicit non-secret fields. These working
tree checks are not Cairn evidence receipts or full commitment acceptance.

| Artifact | SHA-256 |
| --- | --- |
| `freeze4.json` (349 inputs; 18 changed/new files) | `6e6e4109e495d6585863077fabc503334df2f4cf59b290857c6087f28417734d` |
| `full-final3.log` | `ac93b863c279178eaac50f96bec4ea2f9356a421c9bbb66c6cc3eccb93aedcb4` |
| Final main executable | `4a1092e1088ab196093a90788c218ee772b2c8f7bede682e860081dcf7d508fa` |
| Restored Cargo configuration | `03861e19e619274355ae786816cd1a4a1d27ccecd67b38533e04f9c2602f2f0b` |
| Corrected SPEC report | `3341190133ffa736d6952bc9d64bf1ea8004cf56a65a2209c2f1fa77ecd2de1b` |
| Final QUALITY report | `2192684f8655b1db1e8e9e06e48615a0b98eee99fc14a56dd06258d7b467c96f` |
| `evidence-manifest-final.json` (325 artifacts; 46 binaries) | `84fc87d175b097773f859148578e55b52b148757fedac09d5081a3ab82c8e8cf` |

## Production-rule self-audit

| Rule | Assessment |
| --- | --- |
| 1. Understand before editing | Mapped original grant, actual host, exact invocation, persistence, transport and cleanup paths. |
| 2. Smallest coherent change | Reused existing grants, operation ledger, runner leases and service limits. No dependency, configuration or public-admission redesign. |
| 3. Maintainability | Invocation authority, retained service view and cleanup ownership have distinct types and lifetimes. Focused sibling tests hold the controls. |
| 4. Boundary contracts | Original owners, keys, views, credentials and deadlines are checked at durable transitions, sends and accepted replies. Existing command/model assertions remain. |
| 5. Errors and secrets | Unknown effects and exact receipts survive refusal. Evidence uses explicit safe fields; secret-path controls use synthetic fixtures. |
| 6. Security | No task fallback or serialized live authority. Confined immutable stdio views and HTTP destination/redirect protections remain. |
| 7. Survivable state | Existing pending/uncertain records, original settlement and explicit reconciliation remain; stop/restart never reset limits or replay unknown work. |
| 8. Reliability | Monitor/stop ordering, permit release, host drop and partial-startup teardown have observed controls; synchronous-I/O limits remain explicit. |
| 9. Track work | Lifecycle dispatch remains the sole active parent item; this prerequisite is implemented and verified; remaining lifecycle work stays open. |
| 10. Verification | Actual requests, replies, retained state and process disappearance distinguish behavioral failures from fixture/compiler issues. The final full suite, affected tests, Clippy/format checks and separate final-binary PTY ran and passed. |
| 11. Honest reporting | Failed greens, wording/lint failures, nonzero static exits, source variants, ignored tests and pending package work remain visible. |
| 12. Partnership | Followed the recorded lifetime policy and preserved restored Cargo settings; refined the equivalent-alias fixture instead of inventing stricter semantics. |
| 13. Final audit | All 14 rules assessed; both independent reviews pass with no unresolved source finding. Remaining commitment work and evidence limits stay explicit. |
| 14. Plain writing | Records name the original grant, current invocation, observable effects and exact limits of the evidence. |
