# Preserve structured Claude content through post-tool corrections

Level: Judged
Decided by: agent
Rests on: HOOK-004,HOOK-008,PCOMP-002,PRUN-001
Would be wrong if: A structured replacement becomes JSON text, an old cancelled tool identity is reused, or concurrent releases send incompatible search citation settings.

## Decision

Keep the qualified external interruption and correction-allocation protocol. For Claude corrections carrying validated provider content, send a bounded user-content array with separate immutable host evidence and plugin-attribution text followed by the preserved text, image, document or search-result blocks. Do not create a new tool-result envelope or reuse the cancelled tool ID. Bind acknowledgment to the exact typed message and record its representation in the existing lifecycle receipt before uncertain delivery; historical text-only corrections keep their prior decoding. Enforce search citation agreement across possibly delivered content in the same source conversation at both ordinary and correction reservation, including typed corrections and all delivery orders. The retained receipt scan is conservative after compaction. Keep unsupported tool-reference and unqualified file-source outcomes explicit until those source capabilities are qualified. Actual pinned local-peer probes establish typed handoff framing, not live-provider acceptance or remote resource access.

## Realized by

(none yet: recorded, not built)
