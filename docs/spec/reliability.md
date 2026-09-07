# Coding session reliability

Status: Agreed 2026-09-07
Prefix: REL

The developer selected reliability next after the terminal and prompt clipboard
commitments. The four reports are hypotheses until isolated reproductions establish
their current behavior. Preserve normal developer networking and explicit host mode.

[REL-001] Default confined commands MUST deny access to host Unix control sockets outside explicitly allowed session resources.
The check MUST use a harmless private fixture service, never a privileged host service.
Falsifier: Confined Bash contacts the private host socket or confinement is weakened to obtain a passing result.
Mechanism: reliability-sockets; production executor with a disposable service, outside-write/private-file canaries and ordinary network/tool availability regressions.

[REL-002] Full command or event queues MUST NOT freeze editor input, cancellation or quit.
A rejected prompt MUST remain in the editor with a visible explanation.
Cancellation MUST reach owned work within the existing two-second grace period.
Falsifier: A held consumer or saturated queue blocks input or quit, loses a rejected draft, or prevents cancellation from stopping owned work.
Mechanism: reliability-queues; held consumers and real terminal input, queue saturation, cancellation, quit, accepted/rejected prompts and normal continuation.

[REL-003] Anthropic streamed tool arguments MUST enforce a one-MiB byte bound while accumulating input.
An oversized response MUST fail without admitting its tool calls.
Falsifier: Accumulation crosses the bound before block completion, waits for the stream to finish before rejecting excess, or an oversized call has an effect.
Mechanism: reliability-streams; controlled held streams at and beyond the limit, split fragments, actual native adapter events and absence of tool effects.

[REL-004] Tool output MUST preserve valid UTF-8 characters split across reads.
Stdout and stderr MUST have separate decoding state.
An incomplete trailing sequence MUST become a replacement character when its stream ends.
Falsifier: Valid split characters become replacement characters, one stream completes another stream's bytes, or retained and displayed output disagree.
Mechanism: reliability-streams; every split boundary, interleaved stdout/stderr, invalid and incomplete sequences, production Bash output, retained receipts and cancellation.

Each mechanism must demonstrate a safe violating example and its correction or
record why a reported issue does not reproduce. Source review alone is not proof.
Prompt editing, general Markdown, search, hash edits, persistence and subagents
remain later work. No live-provider calls or external service probes are required.
