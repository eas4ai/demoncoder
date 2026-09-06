# CODE-003 mechanism review

The production terminal and event paths already stream assistant and Bash
output while accepting editor input. This action adds the missing check.
`tests/responsiveness.py` runs all four adapters in real pseudo-terminals.

The peer first sends an assistant delta and holds the rest of its response.
The driver must observe that delta, type a draft, and observe the draft in
the editor before it releases the peer. It also checks that no tool or
terminal outcome has already been recorded.

The next request runs actual Bash, which prints a marker and waits for a
release file. Before creating that file, the driver must observe the tool
output and another typed editor suffix. The retained stream must have tool
output but no tool result or terminal outcome yet. Only then does the driver
release Bash, await the final response, and shut down the application.

All four corrected cases passed. With `--fault-buffer-assistant`, each peer
withholds its first delta until release. All four cases fail at the first
pre-release checkpoint, showing that buffered completion cannot satisfy
the mechanism. The fault does not change application code or touch files
outside temporary repositories.

This establishes controlled incremental output and responsive input. It
does not establish steering submission, cancellation timing, live provider
latency, or behavior for arbitrary output volume; those are separate checks
or limits. Each checkpoint has a three-second local deadline.
