# Retain abandoned TLS handshakes separately from application failures in controlled peers

Level: Judged
Decided by: agent
Rests on: HOOK-004,HOOK-005,PCOMP-003
Would be wrong if: A malformed request, TLS protocol error, delivered model request or failed model response becomes a passing boundary check, or disconnect evidence is lost.

## Decision

Actual cancellation phase probes show a valid CONNECT followed by SSLEOFError inside the TLS handshake, before the model handler starts, with the host cancellation assertions passing and zero model requests. The controlled HTTPS peer will retain only exact SSLEOFError and ConnectionResetError raised in that handshake phase as bounded persisted abandoned-before-HTTP observations. It will not infer why the peer disconnected. The same exceptions in request, headers, CONNECT reply or model handling remain failures, as do other TLS errors, malformed/truncated frames, wrong targets, journal overflow and journal write failure. Retain sequence and monotonic time without peer payloads. Settle fixture connection handlers before final evidence inspection so late errors cannot be missed. Model request counts, actual source callback ownership, cancellation, receipts and correction assertions remain unchanged. Demonstrate both allowed disconnect types and the same failures at every other phase, plus protocol and persistence failure controls; rerun affected actual host and source checks. This is a controlled test transport correction, not permission for production callbacks to bypass acknowledgment.

## Realized by

- c8d06d72079412bdc13c909f31015102dfa3f749 Bind managed Codex Submit and Stop to durable lifecycle owners
