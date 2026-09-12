DECISION

Question:   Should we revise the frozen profile so imported Codex SessionEnd MCP hooks are marked source-nonexecuting and require explicit native conversion to run?
Recommend:  Correct only the Codex SessionEnd/mcp_tool applicability cell, update its conformance cases and specification wording, preserve command hooks, and deliver MCP shutdown execution through the existing explicit Convert to native flow.
Because:    The agreed profile says run, but original Codex revision 3d2ee51ca2d5db578f328aa75e20aa22c0197c9a, codex-rs/hooks/src/engine/discovery.rs lines 582-599, skips SessionEnd MCP hooks with a warning; its test at lines 1048-1081 asserts they do not execute. The fetched original file matches the managed source byte-for-byte, SHA-256 fd05ee932079df2b5128f170beaf0d8e5c8630a56eadbcf9de9f25806e772b04. Source inspection and byte comparison ran; the upstream test was inspected, not executed.
If wrong:   If another supported upstream path executes these hooks, marking them nonexecuting would unnecessarily require native conversion. The finding is limited to the exact pinned revision and handler pair.
Instead:    Keep automatic execution as an intentional DemonCoder behavior difference, explicitly revise the contract to name that difference, and test it without claiming upstream Codex compatibility. Neither choice removes MCP shutdown functionality or reduces the complete commitment.

Reply: ok | instead | ask. If this isn't clear, ask me to explain it another way before you decide.

Concerns: PCOMP-002,PCOMP-004
Status: open
Raised: 2026-09-11T01:59:46.174Z
Raised after: PCOMP-002=1 PCOMP-004=1
Answer: ok
Answered: 2026-09-11T10:00:40.364Z
Answered after: PCOMP-002=1 PCOMP-004=1
Answered order: 6
