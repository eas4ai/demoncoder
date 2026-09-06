# Developer usability

Status: Agreed 2026-09-06
Prefix: USABLE

The developer selected these corrections after using DemonCoder on its own
repository: a scrollable virtual chat window with bounded terminal memory, and
ordinary coding access that does not prevent searching, reading documentation,
reviewing repositories, or establishing verification evidence. The developer
explicitly rejected the earlier overly restrictive read/network boundary.

[USABLE-001] The chat MUST support scrolling through retained conversation with page keys and the mouse wheel, returning to the latest output, and continuing to receive output while the developer reads earlier content. Rendering MUST process only the visible rows after layout is cached, with bounded transcript and layout memory and an explicit indication when older history has expired.
Falsifier: Scrolling does not change the visible conversation, arriving output jumps a scrolled viewport to the bottom, memory grows without its documented bound, empty chunks evade the bound, or every idle frame rebuilds and wraps the entire transcript.
Mechanism: developer-usability; production transcript/layout tests and a real-terminal driver covering history, resize, incremental output, controls, and bounded retention.

[USABLE-002] Ordinary Bash MUST work in a repository containing normal build hard links and reference symlinks. Unsafe writable aliases MUST affect only their own paths, never prevent unrelated commands such as pwd or Git status. Outside-workspace writes MUST remain blocked in the confined default, with explicit exceptions only for session scratch and documented tool caches.
Falsifier: An unrelated link blocks every command, a command changes an unauthorized outside canary through a link, a protected settings store becomes accessible, or confined startup silently falls back to host execution.
Mechanism: developer-usability; real Bash checks in disposable repositories containing internal links and outside canaries, plus a read-only check in the developer-selected DemonCoder repository.

[USABLE-003] Ordinary sessions MUST be able to read source and documentation outside the selected workspace, use local developer tools, inspect Git metadata and Cairn evidence, and contact documentation, Git, CI, and dependency services over the network. Provider credentials and known private credential stores MUST remain protected. Missing tools, authentication, or service access MUST produce a specific limitation instead of fabricated evidence.
Falsifier: The default blocks harmless outside documentation or machine standards, installed tools cannot start solely because their paths are hidden, all networking is disabled, or a check is reported as current without executing it or checking its recorded inputs.
Mechanism: developer-usability; inspect known documentation through native read and Bash, run installed tool versions and repository diagnostics, exercise network access against a controlled endpoint, and inspect protected canaries and sanitized environments.

[USABLE-004] The terminal and tool descriptions MUST explain the actual default access and scrolling controls. Automated evidence MUST distinguish local checks, service-backed observations, and checks unavailable for a concrete reason.
Falsifier: The app claims the default has no network or cannot read outside the project, the developer cannot discover scrolling controls, or documentation presents unavailable/private CI or historical receipts as a current passing result.
Mechanism: developer-usability; compare production descriptions and terminal controls with behavioral checks and retain a review of actual self-repository observations.

The chat retention bound limits display memory, not the model provider's context
or every process allocation. Durable recovery, model-context compaction,
cumulative budgets, subagents, and advanced orchestration remain later roadmap
commitments. Host --yolo remains a separate explicit write-access choice.
