# Reliability mechanism and implementation review

Status: in progress

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
