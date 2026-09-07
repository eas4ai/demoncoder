# Chat presentation

Status: Agreed 2026-09-07
Prefix: CHAT

The developer requested meaningful response markers, compact long output with a
keyboard expansion shortcut, code highlighting and about 10–15px of right margin,
using the supplied Codex screenshots and local Codex/OMP sources as references.
Terminals use character cells; the right gutter is two columns.

[CHAT-001] The chat MUST distinguish user prompts, assistant responses and tool activity with meaningful markers and labels. Tools MUST show their operation and target with truthful running, successful, failed or interrupted state. Streaming output and its final receipt MUST not duplicate the same output in the chat.
Falsifier: Assistant and tool output remain an undifferentiated text wall, a tool is marked successful before its result, its command/target is absent, or streamed output is repeated in its final block.
Mechanism: chat-presentation; production terminal cases for assistant text and successful, failed and interrupted tools, including streaming and final results.

[CHAT-002] Long assistant and tool output MUST have a compact preview after wrapping, with an explicit hidden-row count and Ctrl+O expansion hint. Ctrl+O MUST toggle all retained output between compact and full views without modifying provider input, tool execution or event evidence. Existing scrolling, history anchors, resize, cancellation and input during work MUST remain usable. Retention MUST stay bounded and expired output MUST remain visibly identified.
Falsifier: One long wrapped line floods the compact view, retained middle content is unreachable after expansion, toggling loses text or submits a prompt, incoming output moves an anchored reader, or retained text/index memory grows without bound.
Mechanism: chat-presentation; deterministic layout/retention tests and current-screen keyboard, resize, streaming and continuation cases.

[CHAT-003] Supported fenced code and source-file read output MUST have syntax highlighting while preserving literal text and Unicode wrapping. Unknown languages or oversized highlighting inputs MUST remain readable as plain text. Untrusted output MUST NOT execute terminal control sequences. The terminal MUST reserve a two-column right gutter when space permits, without failing at tiny sizes.
Falsifier: Known code has no syntax colors, styling changes the underlying text or wrapping, unknown-language output vanishes, an escape sequence controls the terminal, or normal content reaches the right edge despite adequate width.
Mechanism: chat-presentation; rendered-cell/style assertions and actual terminal screens for code, Unicode, control characters, fallback and narrow layouts.

[CHAT-004] The chat MUST show a scrollbar when the current compact or full view exceeds the viewport. Its position and thumb MUST follow the current view and scroll anchor, including after resize and expansion. Two empty columns MUST remain outside the scrollbar when space permits.
Falsifier: A long view has no scrollbar, the thumb reports the wrong position, switching compact/full views leaves stale geometry, or the rail consumes the requested outer margin.
Mechanism: chat-presentation; rendered-cell assertions and current-screen scrolling/resize cases.
