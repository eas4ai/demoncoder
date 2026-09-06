# Bounded scrollback mechanism review

USABLE-001 and USABLE-004; 2026-09-06.

The previous view accumulated 100,000 empty chunks in a failing unit test. Its
real terminal failed Page Up and the history-expiry indication. The replacement
passes those cases, coalesces fragments, bounds text to 1 MiB and logical lines
to 16,384, and retains wrap offsets in bounded per-line vectors. Tests inspect
retained bytes, entries, allocated offset capacity, and actual visited rows and
lines. Idle layout examines zero text bytes; rendering visits only the requested
24 rows. Incremental tail layout does not rescan an 800,000-byte prefix.

The production tests also cover more than 65,535 wrapped rows, oversized Unicode,
fragmented combining sequences and emoji, resize, and expired anchors. The PTY
driver observes the current screen, exercises page keys, wheel, Home and End,
receives more output while scrolled, types during streaming, resizes down to a
4-by-8 terminal and back, and checks that mouse capture is restored on exit.
Individual key events are spaced during the tiny-terminal check; a combined
escape-sequence burst was not reliably parsed by the terminal input library.

Executed: cargo test --locked --lib (13 passed), python3 tests/scrollback.py
(2 passed), and python3 tests/usability_contract.py (1 passed). The latter uses
actual tools and controlled network responses, including failing CI and stale
receipt observations. It does not claim public-service verification.

This bounds retained display state. It does not bound provider conversation
context, add durable session history, or establish the cause of the reported
terminal crash. Resize reflows retained text; ordinary idle frames do not.
