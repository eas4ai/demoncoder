# CONN-006 mechanism review

Examined the agreed usage requirement and falsifier, all four adapter usage
parsers, `View::event`, and the connection envelope written by EventSink.
The driver uses real HTTP streams or backend JSON messages through the
production executable in a pseudo-terminal. Each named connection receives
four consecutive turns: reported values, partial values, explicit zeros,
and absent usage. Each response pauses before publishing its final usage.

The driver reconstructs the current screen rather than searching historical
terminal bytes. It requires unknown usage while the response is held, then
checks the completed turn's exact counts and cost. Retained events must have
the selected connection and exact optional dimensions. Distinct values for
each adapter help detect crossed records. Claude reports a known price and
an explicit zero price; the remaining adapters keep unknown prices. Codex's
absent case sends no usage notification at all.

## Observed failure and correction

The first working checkpoint test failed on all four adapters at the second
turn: the footer still displayed the previous turn's counts. Resetting usage
on TurnStarted corrected the failure. All sixteen turn cases then passed.

A separate safe source fault changed the terminal's unavailable-count
formatter to display zero. All four adapter cases failed the partial-usage
footer assertion. Restoring the formatter passed the complete matrix again.
The fault executed no tools and contacted no real provider. The initial
harness draft looked for completion in the footer; completion is in the
header. That harness error was corrected before recording the product fault.

## Limits

This verifies the adapter and terminal treatment of reported data. It does
not establish billing accuracy at a provider, recover usage never sent by
a provider, or provide cumulative session accounting. The footer presents
the latest report in a turn; raw attributed events preserve earlier reports.
