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
