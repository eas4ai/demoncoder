# Isolate model hook backend instructions without relocating subscription credentials

Level: Judged
Decided by: agent
Rests on: HOOK-007,PRUN-002,PCOMP-003
Would be wrong if: A model hook receives ambient instructions or private workspace data outside its admitted snapshot, or isolation changes the selected subscription account or billing route.

## Decision

Run hook model sessions from an owned empty transport directory and expose captured evidence only through host-controlled input and snapshot tools. For Claude use the installed and tested safe mode, which retains subscription authentication and explicit SDK MCP tools while disabling ambient customizations. Extend the pinned managed Codex integration with a host-selected model-hook mode that suppresses ambient instruction loading while retaining the original authentication directory, credential storage and account restrictions. Disable ambient executable customizations and supply fixed hook instructions. Qualify actual backend requests and inspection calls with synthetic local canaries; ordinary sessions retain existing behavior. Do not relocate or copy subscription credentials merely to isolate instructions.

## Realized by

(none yet: recorded, not built)
