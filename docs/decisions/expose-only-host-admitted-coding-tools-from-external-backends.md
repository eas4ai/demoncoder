# Expose only host-admitted coding tools from external backends

Level: Judged
Decided by: Codex
Rests on: Agreed CODE-007 and the shared confined executor decision; installed Codex 0.153.4 supports an empty environment selection
Would be wrong if: An installed backend can execute a built-in or inherited MCP tool outside the host executor, or a supported backend loses ordinary session behavior

## Decision

Run Codex threads and turns with an explicitly empty environment selection so its built-in filesystem tools cannot acquire a workspace. Disable inherited MCP servers before starting the thread, alongside plugins, apps, hooks, and other external tool sources. Claude exposes only the host SDK MCP tools and rejects other tool requests. Keep backend ownership and session identity. Verify the installed backends with controlled local model responses as well as the shared executor canaries; unknown or unsupported capabilities fail closed.

## Realized by

(none yet: recorded, not built)
