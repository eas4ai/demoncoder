# CONN-005 revised mechanism review

Examined the revised model/effort precedence requirement, config validation,
the four adapter controls, and the home/override terminal cases. The seven
invalid-input cases and the corrected four-adapter override tool cycles ran
successfully during the preceding authentication review on the same source.
No application code changed during this review.

Actual request checks cover OpenAI reasoning.effort, Anthropic output_config,
Codex turn/start effort, and Claude command arguments. The deliberate wrong
OpenAI field failed; the restored field passed. Unsupported effort and saved
API credentials on a subscription connection reject before startup.

Mismatch still open: the driver does not yet exercise an independently
registered adapter declaring an unsupported operation or report a CONN-005
result. That work remains a separate implementation action. Backend model
incompatibility also needs a controlled rejection case establishing no
fallback. A reviewed digest acknowledges the revised contract; it cannot
substitute for these missing cases.

## Implemented cases

The registry now accepts explicit declarations for the four coding tools,
steering, and cancellation. It checks those declarations before calling
the selected factory. The original version-1 registration method retains
its full coding-session promise. Explicit declarations default to no
capabilities; all four built-ins declare the full set.

The separate capabilities test registers a fixture missing each control
in turn, selects it through the normal configuration path, and requires
the specific error and zero factory calls. It also checks interface
version rejection, duplicate registration rejection, and successful full
registration. Temporarily omitting the registry's gate made this test fail
because the missing capability was admitted. Restoring the gate passed.

The model-rejection driver exercises all four adapters. Each receives the
selected unavailable model, returns its error, and must finish failed with
no tool, replacement model, or automatic retry. Existing home/override
cases still check model and effort delivery. The rejection matrix,
independent registry terminal case, authentication matrix, and clippy with
warnings denied passed. A declaration is a trusted adapter promise, not a
proof of its implementation; the built-ins also require their behavioral
session checks and live evidence.
