# Clipboard shortcut review

Status: changes required

Initial mechanism review found that the inherited interaction tests did not
exercise prompt copying or bracketed-paste mode. Its first passing receipts
therefore do not establish the new requirements. Added focused prompt-only PTY
checks to the mechanism; both fail on the previous runtime. Copy emits no OSC 52
request, and bracketed paste is not enabled. Implement the prompt shortcuts and
mode lifecycle, preserving the separately tested transcript Ctrl-Y behavior.
