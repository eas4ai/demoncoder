# Clipboard shortcuts

Status: Agreed 2026-09-07
Prefix: CLIP

The developer corrected the clipboard shortcuts to Ctrl+Shift+C for copy and
Ctrl+Shift+V for paste, in the prompt editor only. Plain Ctrl+C remains cancellation.

[CLIP-001] Forwarded Ctrl+Shift+C MUST request copying the current prompt text.
Plain Ctrl+C MUST retain cancellation behavior.
The UI and controls documentation MUST name Ctrl+Shift+C and Ctrl+Shift+V.
Falsifier: Forwarded copy cancels a turn, the controls show the wrong keys.
Mechanism: clipboard-shortcuts; production PTY copy events and cancellation regressions.

[CLIP-002] Terminal-delivered bracketed paste MUST enter bounded editor text without submitting a turn.
The application MUST enable and restore bracketed paste mode.
Falsifier: Pasted newlines submit prompts, pasted control characters invoke commands, or exit leaves bracketed paste enabled.
Mechanism: clipboard-shortcuts; real terminal paste events, input bounds and exit sequences.

Terminal emulators commonly intercept these shortcuts. Ctrl+Shift+V delivers the
clipboard through terminal paste; the application does not read the system
clipboard. Copying the application prompt requires Ctrl+Shift+C to be forwarded as a
distinguishable key event. Terminals that reserve it use their own native selection
and copy controls; the application cannot override that interception.
