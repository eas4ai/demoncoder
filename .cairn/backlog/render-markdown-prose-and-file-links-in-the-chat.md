# Render Markdown prose and file links in the chat

Surfaced from: CHAT-003
Captured: 2026-09-07T01:51:07.383Z

The developer supplied a screenshot with visible bold markers and raw file links, then tui-syntax-highlight, Leaves, and ratatui-markdown references. The current chat commitment preserves literal Markdown outside highlighted code. A future selected requirement can define readable emphasis, lists, tables, and file-link display while preserving streaming, text retention, logical anchors, and safe terminal output. Compare parser-only reuse before adopting a whole preview widget; both supplied Markdown libraries currently target Ratatui 0.29.
