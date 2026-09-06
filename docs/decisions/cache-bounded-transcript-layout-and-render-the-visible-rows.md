# Cache bounded transcript layout and render the visible rows

Level: Judged
Decided by: Codex
Rests on: USABLE-001 and the developer request for a scrollable virtual chat window with bounded memory
Would be wrong if: The index or empty entries grow without bounds, idle rendering scans the full transcript, or incoming output moves a scrolled viewport

## Decision

Retain a bounded recent transcript with explicit expiry, coalesce streaming fragments into logical lines, cache wrapping as byte offsets, and use an ordered row index to locate the visible window. Bound both text bytes and line/index entries. Keep viewport state separate from live-tail following, preserve it across new output, and reflow on terminal resize. A bounded retained transcript does not require a second LRU cache or a disk-backed session store. Test scrolling, resize, Unicode, retention, empty deltas, and rendering work through the production module and a real terminal.

## Realized by

(none yet: recorded, not built)
