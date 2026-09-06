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
