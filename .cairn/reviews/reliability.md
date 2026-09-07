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
