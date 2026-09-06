# CODE-007 mechanism review

Reviewed 2026-09-06 against the agreed final-admission requirement and
falsifier. The mechanism runs Rust executor checks and drives the production
terminal and all four adapters in temporary repositories.

The direct API paths receive controlled HTTP responses. The subscription
paths launch installed Codex 0.153.4 and Claude Code 2.1.263, with temporary
homes, synthetic authentication, and local model endpoints. These are real
backend binaries, not protocol stand-ins. Each model request must advertise
exactly the four host tools. The model requests permitted read, write,
edit, and Bash operations, then unauthorized sibling access and protected
Git writes. Bash must not receive provider credentials or see the fixture
home or sibling. The driver checks actual results, final files, unchanged
canaries, and absence of unauthorized markers.

Forced unadvertised Codex apply_patch and exec_command calls, and Claude
Read and Bash calls, must return unsupported-tool errors. Neither may write
the harmless sibling marker. A configured inherited Codex MCP server has a
harmless startup marker; it must never start. The allowed operation cycle
also detects a backend configuration that disables the required host tools.

The shared executor tests deny a marker write through a hook, change an
admitted path to a denied path, and change a Bash command to a sibling write.
They check that transformed arguments face the final boundary and original
arguments do not execute first. A permitted transformed command must succeed.
Existing cases also reject symlinks and hard links without changing canaries.

Failure demonstrations executed:

- The installed Codex check initially exposed built-in apply_patch, inherited
  MCP tools, and orchestrator skill tools. Restricting environment selection,
  disabling each inherited server, and disabling other tool sources removed
  those paths. Forced built-in requests then returned errors.
- The actual Codex tool cycle exposed an inbound tool-request ID equal to
  the pending turn/start request ID. Treating only messages without a method
  as replies corrected the collision and allowed the full cycle to finish.
- Temporarily omitting the denial hook in the Rust fixture caused the
  harmless marker write to succeed. The normal denial assertion failed.
  Restoring the hook passed all three executor tests.

The boundary driver reads complete retained event lines while draining the
terminal. It requires a completed turn and the model's final response after
all tool results were checked. This avoids mistaking terminal redraw byte
fragments for a failed boundary. Separate CODE-001 and CODE-003 cases check
terminal rendering and responsiveness.

This establishes behavior for the installed versions and controlled model
requests. It does not prove all future backend versions, live authentication,
live service compatibility, crash recovery, or arbitrary kernel resistance.
Live two-turn transport evidence remains required by the connection check.
