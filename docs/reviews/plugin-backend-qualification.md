# Plugin backend compaction qualification

This records the backend foundation for the selected skills-plugins-hooks
commitment. The full dispatcher, runners, package activation, management controls
and remaining lifecycle operations are still implementation work. These probes
do not establish the complete 41-requirement commitment or provide Cairn receipts.

## Actual backends and controlled model routing

Claude Code 2.1.267 and the source-built Codex 0.153.4 managed integration run
through the production adapters and actual compaction operations. Disposable
homes, synthetic authentication and local model peers isolate these probes.
They are actual-backend qualification with controlled model transport, not live
provider or real-account evidence. The Codex source, patch, build instructions,
licenses, executable digest and backend-only results are retained under
`backend-integrations/codex/`.

The host registers only its own compaction callbacks. Package code will execute
through the shared host runners; these bridges do not activate ambient backend
packages. Codex retains ordinary upstream behavior outside explicitly managed
mode. Managed startup checks the required declaration and keeps it immutable.

## Faults challenged

- Manual and automatic compaction: allow, typed denial, handler failure, timeout
  and post-compaction failure. Model request counts distinguish denied compaction
  from completed compaction whose continuation remains held.
- Codex host transport: forged token, unrelated turn, duplicated acknowledgment
  bytes and channel closure at both pre- and post-compaction waits. Invalid input
  must not reach the host handler. Invalid acknowledgment must not release the
  backend. A duplicate JSON frame produces a stopped backend turn rather than an
  adapter transport error; actual model effects establish that it remained held.
- Codex callback retries: the same actual request is sent twice over the private
  channel. Both responses must match, while the host handler executes only once
  and the admitted compaction occurs once.
- Codex backend-only relay: allow, deny, process death, timeout, stale challenge,
  empty/plain/malformed output and nonzero exit, before and after compaction.
  The owner and backend remain alive while their model effects are checked.
- Claude: kill or disconnect the relay while its owner remains alive; separately
  kill the actual Rust owner while the SDK waits for its callback. Both manual
  and automatic compaction must retain the four setup model requests without a
  new compaction request. The supervisor may immediately stop the backend after
  a transport fault, so tests establish liveness before injecting that fault.
- Controlled lifetime cases: owner death, supervisor death, incomplete backend
  JSON and a full response pipe while the gate remains pending. Cleanup must
  stop the backend before closing the last protected stdin writer.

## Review findings and corrections

1. A manual Codex start reply can follow `turn/started` without containing a turn
   ID. Clearing the known ID then stranded the relay. The controlled reordered
   reply case failed before the adapter retained the known ID and rejected an
   inconsistent returned ID.
2. Cleanup originally awaited language-service shutdown before stopping the
   backend. An injected cleanup error left it alive. Both adapters now stop the
   backend first, retain both cleanup errors, and the negative test passes.
3. A model tool could read the Codex relay token from its temporary directory.
   The canary regression failed before moving storage into the protected home
   directory and registering its root with the rebuilt executor. The regression
   checks native read and Bash through current and earlier confined sessions.
4. Claude SDK EOF could release a pending callback after owner SIGKILL. Two
   independently held stdin writer leases now survive either owner dying alone.
   The surviving owner kills the backend group before releasing its lease.
5. Supervisor heartbeats filled the response pipe and killed a healthy owner
   after 22.51 seconds during a valid 60-second gate wait. A validated Linux pidfd
   now reports owner death independently of output backpressure. A 4 MiB output
   flood remains blocked behind a live gate, then owner death stops the group.
6. A raw descriptor duplicate leaked a stdin writer into the actual backend.
   The new descriptor-inventory regression failed before a close-on-exec clone
   replaced it. The backend must have no write descriptor for its stdin pipe.
7. A FIFO hook source hung managed Codex startup before the size bound applied.
   The old artifact exceeded the independent two-second deadline. The rebuilt
   artifact rejects nonregular sources before reading, with nonblocking/no-follow
   opening and descriptor validation. FIFO, directory and symlink cases pass.

8. New model-peer readiness reads could wait indefinitely before the test deadline.
   They now use bounded asynchronous reads with a ten-second deadline and reject
   oversized or unterminated frames. The affected actual-backend probes pass.

## Executed development checks

- Managed Codex: 179 hook tests, 36 backend-only fault cases and 17 startup cases.
- Production Codex adapter: 10 allow/deny/failure cases, 16 wire-fault cases and
  four callback-retry cases against the rebuilt artifact.
- Production Claude adapter: 18 allow/deny/failure/relay-loss cases and two
  actual owner-SIGKILL cases against installed Claude 2.1.267.
- Four controlled lifetime cases and three controlled Claude bridge cases pass
  after the descriptor-ownership correction. Controlled Codex transport cases
  also cover manual reply ordering, qualification rejection and credential access.
- All-target Clippy and formatting checks passed. The final Rust regression
  passed 347 tests across 30 suites; 16 installed/live or subprocess fixture tests
  were ignored by that ordinary invocation. The compaction qualification tests
  above were invoked separately. The later readiness-only fixture change was
  checked by rerunning the affected installed backend probes.

## Limits and review interpretation

Confined credential-denial tests do not establish a filesystem boundary for the
explicitly unrestricted host mode. That mode retains its existing Oracle judgment
and unsandboxed execution contract. Command hooks remain subject to their separate
mandatory confinement, including when the main session uses host mode.

The Claude lease design covers death of either owner separately. It does not
claim that simultaneously destroying both lease holders while sparing the backend
preserves their leases. Process-group cleanup and the remaining owner provide the
tested failure boundary. A full-machine failure has no surviving running backend.

Ripwire's edit check found no incompatible caller for the new Codex callback.
Its quality delta reported increased event-loop complexity/length, public surface
and churn; its test gate reported test obligations and statically untraced edges.
These are inspection reports, not passing behavioral checks. The event-loop
additions implement the required synchronous waits; changing their layout merely
to lower a metric would not establish safety. Rust tests, installed backend
probes and independent specification/quality review supply the bounded evidence.
