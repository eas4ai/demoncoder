# Load private home settings with explicit connection and effort precedence

Level: Judged
Decided by: Codex
Rests on: The developer directed environment or home-directory API credentials, saved model assignments, and thinking effort while answering the CONN-001 escalation
Would be wrong if: A saved key is disclosed to tools or logs, a selected authentication method changes, or a configured model or effort is silently ignored

## Decision

Load a private TOML settings file under ~/.demoncoder/ by default, with --config selecting an explicit trusted file. Keep named connection assignments and their model and effort in that file. Environment API keys override saved keys; explicit CLI connection, model, and effort override saved settings. Validate effort for the selected adapter and send it through the actual provider or backend control. Require owner-only permissions for files containing API keys and never serialize secrets into session events. Preserve the existing older config format while its coexistence choice is pending. Custom encryption and later subagent role orchestration are not introduced by this change.

## Realized by

- b72536233a72739b9c23e386747ec0a88846997d Load private home credentials and pass selected model effort to each connection
