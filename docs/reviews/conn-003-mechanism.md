# CONN-003 revised mechanism review

Reviewed the revised requirement and falsifier, the connections declaration,
check-connections.sh, config loading and API credential precedence, native
HTTP headers, subscription subprocess environment, and the controlled
terminal/configuration cases. No application code changed during this review.

The private-file cases reject an API key attached to a subscription choice,
missing credentials, and an explicitly blank environment key. The corrected
four-adapter override tool cycles passed and establish the actual selected
header and model/effort route. The subscription peers assert both API-key
environment variables are absent. These cases ran during this review and
passed; the earlier deliberate dropped-effort mutation was also detected.

Mismatch still open: the connection mechanism does not yet exercise an
expired API credential, a backend returning the wrong authentication kind,
or prove that those failures make no fallback request. It must not report
CONN-003 passing before those cases are implemented. The mechanism currently
emits no CONN-003 result, leaving this requirement unverified. This review
acknowledges the revised contract; it is not evidence of completion.

## Implemented cases

The mismatch above is resolved by `tests/authentication.py` and its
subscription peer. Both API adapters use distinct environment, saved, and
subscription-shaped fixture credentials. Their actual HTTP headers prove
environment precedence and saved-key selection. Missing API credentials
make no request; a 401 receives exactly one request and no fallback.

Both subscription adapters run with ambient API keys present in the host.
Their subprocesses must receive neither API key. The Codex peer requires
forced ChatGPT login and reports valid, missing, expired, and API-key
account types. The Claude peer reports the equivalent outcomes. Failed
cases emit an error without model success, tools, retry, or fallback.

The first unknown-route Claude case failed: the adapter accepted model
output after an init event omitted its authentication source. Claude now
requires an explicit non-API source before accepting model output,
successful completion, or tool execution. Missing init and a pre-init
write request also fail without a canary effect. Confirmation survives
ordinary turns and is cleared when its backend process closes. The
corrected authentication matrix, continuation cases, and installed-backend
admission checks passed. Clippy passed with warnings denied.
