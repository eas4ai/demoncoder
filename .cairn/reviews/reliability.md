# Reliability mechanism and implementation review

commitment: reliability
commit: c373703d07e7aa860c092f782623792ff6fdf401
findings:
  - none: no open findings within this commitment
Status: complete

## REL-001 mechanism design

A disposable Unix listener under a private home fixture directory is outside the
selected workspace and outside the sandbox's hidden /tmp. The production executor
attempts one connection; the service has no privileged operation. The client must
report denial and the listener must see no accepted connection. Existing tests
retain outside-write/private-file canaries and ordinary IP networking, Git and
installed tool behavior. Baseline and correction results will follow.

### REL-001 failure and correction

The initial committed production-executor fixture reached the disposable host
pathname service and printed HOST-SOCKET-CONTACTED. Cairn retained that failing
baseline (exit 101). After the confined-only filter was attached, the same service
saw no accepted connection. The fixture now catches denial at socket creation as
well as at connection; neither outcome can count a contacted service as a pass.

The corrected local mechanism passed 20 active tests: 15 developer-access cases,
two access unit tests, two socket-filter unit tests, and the explicit-host fixture.
The public-network selected-repository case remains intentionally ignored and is
not counted. Private abstract sockets and datagram socketpair creation are denied;
anonymous stream pairs with socket flags transfer bytes; io_uring_setup returns
EPERM. The emitted BPF is evaluated against foreign/compat architectures, x32
syscalls, integer upper-bit variations and IPv4/IPv6 domains. A real local TCP
endpoint and existing tool, Git, cache, native-read, private-file, outside-write,
TMPDIR, PTY and descriptor regressions pass. The host fixture uses the existing
no-tools Oracle peer and retains Unix socket and datagram-pair access.

The policy lives in a sealed anonymous memory file. Attempts to write or truncate
it, including through a reopened descriptor, fail with EPERM. Repeated command
launches each read the filter independently. The inherited-descriptor check runs
before ctypes loads libffi: checking afterward incorrectly measured a library's
newly opened descriptor, not a leaked filter. No launcher workaround was needed.
The compiler generates native architecture checks; a small explicit x32 guard
precedes it because seccompiler 0.5.0 does not generate that guard. Rust compilation
and clippy passed; final full-tree checks and independent review remain pending.

Sources inspected: rust-vmm/seccompiler (https://github.com/rust-vmm/seccompiler),
its installed 0.5.0 compiler source, rustix 1.1.4 memfd/seal APIs, and local Linux
manuals/headers plus bubblewrap help. The independent socket assessment supports
this boundary; it is not the final fresh implementation review.

## REL-002 mechanism design and baseline

A separate test executable calls the production terminal with a capacity-one
command queue already filled and a live consumer deliberately held. Python sends
real PTY keys, replays the rendered screen, requires a visible full-queue notice
and editable retained draft, then requires quit within two seconds. A separate
cancel-key case checks that cancellation input cannot freeze quit. The initial
local prompt case failed: the terminal took the draft and awaited queue capacity,
so no notice or subsequent frame arrived. This is a real held-consumer failure,
not a source-shape assertion. Runtime cancellation under event backpressure and
normal accepted-prompt continuation still need adding before this requirement
can be considered proved.

### REL-002 correction and integration inspection

The terminal uses an acknowledged Submit command while retaining the original
Prompt variant and Session::turn signature. It keeps one pending draft, accepts
edits during admission, clears only an unchanged accepted draft, and shows a
rejection reason. Cancellation has one nonblocking pending request, and quit is
not awaited in an input branch. Main's shutdown deadline includes queue submission
and aborts/joins a timed-out worker.

The runtime worker reproduced native advisory cancellation blocking beyond 500 ms
and Codex blocking beyond two seconds. An intermediate test also found a dropped
admission receiver consuming a native correction slot, and a drained Codex queue
accepting correction 33. The corrected code replies without awaiting event delivery,
reserves capacity before acceptance, and holds a shared permit across the channel
and adapter-local vector until application/drop. Native and subscription owners
now bound unapplied corrections at 32. Only advisory correction notices can omit
the live copy under pressure; optional logs retain them, while substantive text
and original tool receipts still use awaited publication.

Parent integration inspection then found lifecycle publication outside the
cancellable turn. A new public session::run test failed after 500 ms with shutdown
blocked behind Ready. The corrected helper keeps polling controls while Ready,
TurnStarted, Error or TurnFinished waits for capacity. Additional prompts receive
an immediate rejection, Cancel can skip a waiting turn, and Shutdown closes the
owner. The corrected test covers Ready, start and finish, checks rejection reasons,
checks that cancellation does not start work, and observes owner cleanup without
draining the held event channel. A closed UI receiver is treated as shutdown;
retained-log errors still propagate.

The five runtime cases (including the parent's lifecycle test) pass in
local verification, as do the closed-handoff unit and retained-receipt cancellation
unit. Parent PTY cases exercise full command queues, delayed acceptance/rejection,
edits during admission, repeated Enter, retry and quit. Existing independent
registration, four-connection steering and all eight provider/tool cancellation
cases pass. Additional actual-main quit cases stop owned tools on all four
connections within two seconds. Clippy with warnings denied passes after the
lifecycle correction. The full committed mechanism will supply the receipt.

Limits: synchronous event-log writes can still be delayed by a stalled filesystem;
these checks exercise queue backpressure, not filesystem stalls. Exhaustive matches
in custom Session implementations must handle Command::Submit; this additive enum
change is documented. The Session::turn signature remains unchanged.

## REL-003 mechanism design

A loopback HTTP fixture drives the actual Anthropic native session with multibyte
tool arguments at exactly one MiB and one byte beyond. It sends either several
fragments or one oversized fragment and holds block/message completion. Excess
must fail within one second, with no tool admission or filesystem effect. The
exact-bound case must wait for completion, then write the expected byte count.

### REL-003 failure and correction

The committed baseline failed because the oversized response remained pending for
one second while the fixture withheld block completion. The exact-bound case
already passed. The adapter now checks each fragment's byte length against the
remaining allowance before appending. The check uses subtraction to avoid length
addition overflow. Returning an error at that point prevents response tool
admission. This is the specified per-call argument bound, not a new response-wide
allocation policy.

## REL-004 mechanism design

The production confined Bash executor runs disposable Python writers with delayed
writes at every internal boundary of two-, three- and four-byte UTF-8 characters
on both stdout and stderr. Separate cases interleave partial characters across
pipes and send invalid or incomplete sequences. The test compares each stream,
the live concatenation, the final tool receipt and the retained event log. Small
writer delays encourage separate OS reads; deterministic decoder tests will cover
all boundaries directly once the implementation exists.

### REL-004 failure and correction

The committed baseline rendered a split é as two replacement characters. A
separate case assembled stdout's E2 with stderr's 82 AC into a euro sign in the
receipt while the display showed three replacements. The correction keeps one
incomplete code point per pipe and adds each decoded fragment to both the live
event and final receipt in the same order. EOF replaces an incomplete suffix once.
The existing one-MiB limit still counts raw pipe bytes, so replacement expansion
does not change which commands exceed the limit. Deterministic tests enumerate
every partition of representative valid, invalid, overlong, surrogate and
out-of-range sequences and compare standard-library whole-stream decoding.

## Parent complete-diff review (5d847cf)

Inspected the complete implementation/test/decision/declaration diff from 44487e6.
Attacked filter file ownership and ABI bypass, correction acknowledgement order,
held lifecycle events, permit lifetime after draining, cancellation cleanup, UTF-8
EOF/replacement behavior and argument admission before effects. Original retained
receipts remain on the awaited event path; advisory notices alone may omit a live
copy. The tests preserve the private socket canaries and ordinary networking.

Open finding REL-003: inline input in content_block_start is not checked by the
fragment guard. A disposable loopback/PTY experiment on the actual debug binary
sent a small write followed by an oversized inline write. The small call changed
batch-effect; the oversized call was rejected only by ToolExecutor. This violates
the requirement that an oversized response admit none of its calls. Check inline
input at block start and retain an actual native-session regression before closing
this finding. No source code was changed during this review.

Verification at this candidate: cargo fmt, clippy all-targets with warnings denied,
and cargo test --locked --all-targets passed (86 passed, five driver-only ignored).
All REL mechanisms have passing committed receipts. Startup, developer usability,
chat presentation, sweep interaction/status/investigation/docs scripts passed.
Coding-session local cases passed through host/Oracle fixtures, then its retained
live Oracle check failed as stale. Connections local tool/configuration cases
passed, then retained live records for all four connections were stale. These
older paid-provider receipts were not refreshed; reliability explicitly requires
no live-provider calls. Remaining connection fixture cases will be run separately.
Full script logs: /tmp/demoncoder-reliability-checks-d4qloeg9.

Ripwire with reference/ excluded ran successfully. quality-delta reported zero
working-tree regressions against HEAD; it does not assess already committed
changes. test-gate named 11 harnesses and 21 statically unlinked impacted symbols
(exit 4 is its obligation report). Full Rust tests and the terminal/connection
drivers cover the applicable runtime paths; the live Oracle driver retains the
limitation just described. Complete accumulated-diff review supplies the committed
change assessment that quality-delta cannot provide.

### Inline argument finding correction

The new held-inline regression failed before the fix: the adapter waited for
completion instead of rejecting the oversized block. Both inline and fragmented
fixtures now place a valid write before the candidate call, checking that neither
call has an effect on rejection. The block-start path now validates serialized
input before storing the block; exact-bound input remains accepted. Final review
and fresh independent acceptance are still pending.

## Correction review

The inline input change checks at block start before retaining or returning calls.
Held inline and delta fixtures now include an earlier valid write and prove no
batch effects on rejection; exact one-MiB calls still execute. The full Rust suite
passed again (87 passed, five driver-only ignored), as did fmt and Clippy. All
four REL requirements have fresh passing committed receipts. Authentication,
connection ownership, capability rejection and usage fixtures passed separately.

The additional output-limit PTY regression has a stale width assumption: the
100-column terminal visibly clips out 12000 to out 1, despite receiving the
correct output and status. Its assertions should use the existing resize helper
to fit the full status. Keep all original output-limit assertions unchanged.

## Fresh independent review: fix-first

Reviewer /root/reliability_final_review inspected candidate 70ad705c3 and returned
fix-first. Requested model/effort: gpt-5.6-sol/high; actual model, effort and usage
were unobservable. It found one REL-001 defect: the fixed launcher/bwrap path
preserves unrelated inherited descriptors. Its disposable socketpair probe passed
fd 9 into the namespace and wrote HOST-SOCKET-CONTACTED to the host peer. Seccomp
cannot revoke a socket already opened before sandbox entry. The parent will
reproduce through the production executor, close unrelated inherited descriptors,
and add a regression. No additional defect was found in REL-002/003/004.

The developer clarified that Unix services remaining accessible through explicit
--yolo host mode with Oracle review is acceptable; the confined-only creation
restriction is retained. No release has been installed at this point.

### Inherited descriptor correction

The new Python driver passes a disposable Unix socket at fd 127 and an outside
writable canary at fd 257 into a Rust test driver that calls the production
ToolExecutor. Before the correction, the host peer received
HOST-DESCRIPTOR-CONTACTED. The fixed trusted Bash launcher now closes all ambient
descriptors except 0/1/2 and the policy at 3 before execing bwrap; opening the
policy and closing descriptors fail closed. It uses Bash's variable descriptor
redirection, with numeric validation, without eval or another runtime dependency.
The pinned workspace input and captured output pipes remain available.

## Final acceptance and installation

Parent re-examined the complete accumulated diff and the inherited-descriptor
correction without changing code during review. The launcher closes ambient
descriptors only in the child, preserving the pinned root, captured output and
sealed policy. Numeric Bash variable redirection avoids eval and a new runtime
dependency. The original failure and corrected production-executor fixture are
retained. The inline argument and clipped-fixture findings are also resolved.

A new independent reviewer, /root/reliability_final_recheck, reviewed candidate
c373703d07e7aa860c092f782623792ff6fdf401 and returned ASTRA REVIEW: ship, findings
none. Requested gpt-5.6-terra/high; actual model, effort and usage were unobservable.
It independently reran the inherited-descriptor production fixture successfully.
It accepted REL-002 admission/queue controls, REL-003 inline and fragment limits,
and REL-004 separate stream decoding and receipt agreement. The earlier fix-first
review remains recorded above; it was not reused as the fresh accepting review.

After the descriptor correction, formatting, Clippy with warnings denied, and
the full Rust suite passed: 87 passed and six driver-only entries ignored. The
explicit REL mechanisms run the queue, registry and inherited-descriptor drivers
that need external fixtures. All four requirements have current passing committed
receipts. The selected public-network assessment and paid live Oracle entry were
not rerun. Ripwire edit-check found no incompatible caller; its quality delta
reported no working-tree regression. Its test-gate remains an obligation report,
not runtime coverage proof; the executed tests supply that evidence.

Installed with cargo install --path . --locked. PATH resolves to
/home/shawn/.cargo/bin/demoncoder and its SHA256 matches target/release/demoncoder:
82b08fb61b1274ef59b8fbf9bb5dab2c117f47a25f4b4b78b2c12f4876951c46.
The installed executable reports demoncoder 0.1.2. Controlled production tool
cycles passed on all four connections; five output-limit/clipboard PTY tests
passed; quit stopped owned tools within two seconds on all four connections.
An additional installed-main-process fixture injected the ambient socket and
writable-file descriptors, requested Bash through the Anthropic fixture, and
verified both descriptors were closed with no host effects. Full results are in
.cairn/evidence/reliability-install.log.

The developer accepted Unix-socket access through explicit --yolo with configured
Oracle review. Confined commands continue to deny it. Synchronous event-log disk
writes can stall independently of queues; this is outside REL-002's queue
falsifier. Historical paid-provider/Oracle receipts remain stale and are not
claimed as current live evidence; the commitment explicitly required no such calls.

Final self-audit against all fourteen production coding rules found no remaining
revision needed within this commitment: scope and contracts are explicit; the
changes and dependencies are bounded; boundary failures were reproduced; errors
and credentials remain protected; cancellation and resource cleanup are tested;
code, documentation and recorded checks agree. The independent review accepted
the corrected code, and the installed bytes and actual behavior were verified.

API-equivalent cost receipt: unavailable for parent, both assessments and both
fresh reviewers because native tools expose no observed token usage. No cost or
savings estimate is claimed.
