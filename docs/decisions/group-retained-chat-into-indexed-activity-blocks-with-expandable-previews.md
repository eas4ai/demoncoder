# Group retained chat into indexed activity blocks with expandable previews

Level: Judged
Decided by: Codex
Rests on: CHAT-001, CHAT-002, CHAT-003 and supplied Codex/OMP references
Would be wrong if: Grouping breaks bounded scrolling or hides original output, highlighting blocks input, or markers imply unsupported outcomes

## Decision

Keep the existing per-line transcript index inside bounded activity blocks and add an ordered block index for the viewport. Use labeled role/status markers and update a tool block from its authoritative final receipt. Compact long wrapped output to a head/tail preview with Ctrl+O toggling the retained full view. Preserve logical scroll anchors. Use cached Syntect highlighting for fenced code and source reads, with bounded plain-text fallback and no terminal escapes. Reserve two terminal columns on the right. Reference observations: Codex separates tool headings and previews; OMP uses Ctrl+O, hidden-row hints and explicit tool states. This is a native adaptation, not a copy of reference source.

## Realized by

(none yet: recorded, not built)
