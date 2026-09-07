# Usage display

Status: Agreed 2026-09-07
Prefix: DISPLAY

The developer requested that usage remain hidden until a dimension is known.
The later SWEEP-005 status strip reserves its own row for model/context/Git; this
requirement now governs its token/cost segment.

[DISPLAY-001] The terminal MUST hide the token/cost segment before any usage dimension is reported for the current turn, including when all dimensions are unavailable. Reported zero and partial usage MUST remain visible and truthful. Starting another turn MUST clear the previous usage display. Scroll controls MUST remain visible.
Falsifier: An unknown-only token/cost segment appears at startup or during an unreported turn, reported zero is hidden, previous-turn usage remains while waiting, or scroll controls disappear.
Mechanism: usage-display; inspect current terminal screens for startup, waiting, known, partial, zero and absent usage across all four adapters.
