# Describe the writable host tool workspace accurately to Codex

Level: Judged
Decided by: Codex
Rests on: Agreed CONN-001, CODE-002, and CODE-007; the live Codex run read the seed then refused writes because its advertised policy was read-only
Would be wrong if: The policy change restores a backend built-in or inherited tool path, permits an unauthorized effect, or leaves Codex unable to use the admitted host tools

## Decision

Set the Codex thread policy to workspace-write while retaining explicitly empty environment selections for every thread and turn, disabled inherited MCP servers and other tool sources, and the shared confined host executor. The model must receive an accurate description of the four host tools it is allowed to use. Before another live run, repeat the actual installed-backend catalog, forced built-in request, sibling-canary, credential, and protected-Git checks. No unsandboxed host execution is introduced.

## Realized by

- eb1a9de4fd27d7254e9b9de567c2870404a34d1e Align Codex permission guidance with confined host tools

All four installed-boundary cases passed after this change. The next live
run evaluates whether the model can complete the authorized coding task.
