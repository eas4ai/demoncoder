# Group retained chat into indexed activity blocks with expandable previews

Level: Judged
Decided by: Codex
Rests on: CHAT-001, CHAT-002, CHAT-003 and supplied Codex/OMP references
Would be wrong if: Grouping breaks bounded scrolling or hides original output, highlighting blocks input, or markers imply unsupported outcomes

## Decision

Keep the existing per-line transcript index inside bounded activity blocks and add an ordered block index for the viewport. Use labeled role/status markers and update a tool block from its authoritative final receipt. Compact long wrapped output to a head/tail preview with Ctrl+O toggling the retained full view. Preserve logical scroll anchors. Use cached Syntect highlighting for fenced code and source reads, with bounded plain-text fallback and no terminal escapes. Reserve two terminal columns on the right. Reference observations: Codex separates tool headings and previews; OMP uses Ctrl+O, hidden-row hints and explicit tool states. This is a native adaptation, not a copy of reference source.

## Realized by

- bb1324cfcfad7a9c5fbedaf9473a54910fa14175 Group chat activity with expandable output, code colors and scrollbar

## Additional supplied references

- [ratatui-interact](https://github.com/Brainwires/ratatui-interact): useful interaction widgets; its scrollable strings do not supply our retention or logical anchors.
- [ratatui-kit](https://github.com/yexiyue/ratatui-kit): component framework; adopting it would change the UI architecture.
- [ratkit](https://github.com/Alpha-Innovation-Labs/ratkit): broader widget set targeting Ratatui 0.29.
- [ratatui-code-editor](https://github.com/vipmax/ratatui-code-editor): editor behaviors and Tree-sitter grammars exceed the current read-only code-display need.
- [ratatui-cheese](https://github.com/shashanktomar/ratatui-cheese): reusable interactive controls, not the retained chat index.
- [tui-syntax-highlight](https://github.com/aschey/tui-syntax-highlight): Syntect-to-Ratatui code blocks; this implementation uses Syntect directly to retain incremental parser state and bound work.
- [Leaves](https://github.com/freepicheep/leaves): full Markdown renderer, including tables and diagrams. Its current manifest uses Ratatui 0.29 (this application uses 0.30), and its parser returns a complete vector of rendered lines. General Markdown rendering remains a separate extension; no source from these libraries was copied.

- [ratatui-markdown](https://github.com/celestia-island/ratatui-markdown): optional Markdown, scroll, tree and per-language Tree-sitter features. Current manifest targets Ratatui 0.29 and declares SySL-1.0; its displayed LICENSE does not include application reuse terms. Not adopted.

These are component evaluations, not runtime validation of those projects.
