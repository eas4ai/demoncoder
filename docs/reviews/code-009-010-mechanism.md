# Setup and host-access mechanism review

The checks drive production setup and tool admission. They inspect saved
settings, terminal state, actual disposable files, tool results, review
requests, process completion, and distinct Oracle events.

The initial hidden-key test exposed an input race: the prompt appeared
before terminal echo was disabled. The synthetic credential became visible
and the test failed. Enabling raw input before printing the prompt fixed
it. The same test then passed, along with cancellation restoring ECHO and
ICANON without saving and denied trust leaving settings unchanged.

The initial host scratch test found directory mode 0775 instead of 0700.
Explicit private permissions corrected it; the assertion then passed.
Host execution checks the actual project working directory, private TMPDIR,
and absence of the synthetic API credential. Every coding connection uses
the shared executor. Native OpenAI and Anthropic Oracle peers cover allow,
deny, malformed output, unavailable service, and attempted tool execution.
Codex and Claude protocol peers require an empty tool catalog, use their
subscription route, and cover allow, deny, malformed output, and tools.

Rust tests hold reviews open, cancel them, replace existing or new file
targets, and inspect unchanged canaries. They verify hooks are judged after
transformation, native scratch writes need no review, ordinary host child
processes stop on exit and cancellation, and closed output does not bypass
cleanup. The timeout case waits the real 60 seconds and requires no effect
and a stopped Oracle process.

For an explicit admission failure demonstration, the new-file review call
was temporarily omitted. The hook test then failed because its transformed
outside write succeeded despite the denying Oracle. Restoring the review
made the same test pass. The only effect was a disposable canary file.

The live driver requires committed inputs, the default provider endpoint,
and a selected real Oracle. It retains an allowed disposable outside-read
verdict and a denied home-move verdict. Both are proposals only. The
mechanism rejects missing or stale records. These checks establish the
guard's wiring and fail-closed behavior; they cannot prove that a model
will judge every arbitrary shell program correctly or stop a deliberately
detached process. Host access is explicitly unsandboxed.
