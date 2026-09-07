# Usage display review

commitment: usage-display
commit: 58a0ae66fba30844173e96ff064942207c8c39ac
findings:
  - DISPLAY-001: the inherited usage test requires the old unknown label and does not prove the new visibility contract

The baseline passes existing usage and scrollback tests, but the test explicitly
expects usage unknown while waiting. This is not passing evidence of the new
requirement. Update its current-screen assertions to require the editor border
immediately above scroll controls when usage is absent, then demonstrate failure
on the current runtime and success after the visibility correction.
