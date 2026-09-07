# Make task and agent status and decisions readable in the terminal

Surfaced from: SWEEP-005
Captured: 2026-09-07T22:41:25.480Z

The developer requested existing-project reconnaissance after discussing developer experience. At 42b0c0c, src/terminal.rs:773 prints agents 0; actual agent state appears as transcript notices, and src/subagents/session.rs:128 prints the entire record as JSON. docs/recon.md records cited observations and docs/proposals/status-and-decisions.md proposes accurate live counts, readable evidence, available commands and bounded responsive inspection, with falsifiers. Live advisory events may be omitted, so refresh from authoritative state. The roadmap already selects evidence-based improvement next; retain that order until the developer explicitly selects this slice. Also record the stale README wall-clock-budget claim at line 1088 and installed spec-lint findings for ORCH-006, ORCH-007 and SUB-007. No runtime or Agreed specification changes are authorized by this backlog entry.
