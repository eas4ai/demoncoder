# HTTP hook runner review

Status: Approved by independent specification and quality reviews. The empty-authority finding is closed. This bounded PreToolUse prerequisite does not complete the plugin commitment.

## Scope

The candidate registers explicit host-bound Native and Claude HTTP PreToolUse handlers through the existing dispatcher, allowance and durable receipts. It adds bounded POST exchanges, literal source input, separate credentials, secret-safe response retention, owned cancellation and separately admitted read-only revalidation. Other lifecycle events, public activation and configuration, MCP and service integration remain in the selected complete commitment.

## Closed specification finding: empty written URL authority

The independent reviewer reproduced a request from a malformed host binding: `http:///127.0.0.1:PORT/hook/...`. The validator finds the `://` separator but permits the written authority to be empty. The URL parser then normalizes the path into a host. Through the production ToolExecutor, the endpoint receives one POST and the guarded write succeeds. This violates the bounded requirement to accept an explicit absolute authority and reject normalization that supplies a missing authority.

The unchanged independent harness is `/tmp/plugin-http-spec-controls.rs`; `/tmp/plugin-http-spec-empty-authority.log` records actual traffic count one where zero is required and the successful write. The initial candidate source manifest is `/tmp/demoncoder-http-verification/source-manifest.json`, SHA-256 `b49ef96eb5b98d5d18ac66502d3baeac87f6ee8b067af21470c7fa9807f7bc1e`. All five source hashes matched before review. The HTTP source hash is `514772d06067e4c9cd3c73c3169b6f899cab9d12e2222b51560a5a4ea3754c62`.

The correction requires a nonempty written authority before URL parsing and covers extra-slash variants. It preserves valid path/query semantics, secret-safe errors and zero traffic on invalid registration. The same independent control passed against corrected source. No code changed during the independent review.

## Verification before correction

The parent verified all 33 artifacts in `/tmp/demoncoder-http-verification/evidence-manifest.json`, SHA-256 `e6fc440b2c5723e96d44b6afc850e073685a1051d194a1d01bad4f7e0e5319f6`. Final HTTP 22, other affected integration tests 124, and runner unit tests 12 passed. Formatting and all-target Clippy passed. Four meaningful mutation controls failed with redirect, streamed byte limit, decoded-secret or raw-secret protection removed; restored HTTP controls passed. Those tests did not cover the newly found empty-authority case.

The parent separately ran `python3 tests/registry.py` successfully: configured independent provider registration, native read and terminal rendering passed. Log: `/tmp/plugin-http-parent-registry.log`. This executes the pseudo-terminal case ignored by the bare registry_driver target. No formal Cairn evidence or live-provider coverage is claimed.

Static quality-delta remains exit 2 with 72 findings and 35 gating rows; test-gate remains exit 4 with three named targets and fourteen unmodeled symbols. The worker retained every row and proposed dispositions in its handoff. Independent quality assessment accepted the documented design; these are not clean static results.

## Verified authority correction

The implementation now rejects an empty written authority before URL parsing. Two new controls cover primary and read-only bindings across HTTP/HTTPS with three, four and five slashes. Both failed before correction and passed afterward; the full HTTP suite passed 24 cases. Formatting, all-target Clippy and whitespace checks passed. The parent independently checked all five corrected source hashes and sixteen correction artifacts.

The corrected source manifest is `/tmp/demoncoder-http-authority-correction/source-manifest.json`, SHA-256 `d7728c909d7267f682bbceb70610c1880295fbebb0a9a2ccdab6d6cd34041b9c`; its evidence manifest SHA-256 is `ce3b7faf0400ba531550bd30cffc58d609045c21b287f10224121f7f935280d1`. HTTP source is `e0c00ee0a5fcd494120becc609ff37c252974493130bbf739bbe149b2264fc51`. The other three runner source files are unchanged; only HTTP validation and its tests changed. The independent reviewer rebuilt the exact original external control against the corrected library: one test passed, preventing the original unauthorized request/write. The reviewer also reran all 24 HTTP tests successfully and verified the original harness, original failure log and corrected source hashes. The specification review approved the bounded prerequisite; `/tmp/plugin-http-spec-review.md` records the current verdict and historical finding.

Corrected static analysis remains nonzero: quality-delta exits 2 with 74 findings and 35 gating rows (two additional test-entry rows); test-gate exits 4 with the same obligations. Independent quality review approved the bounded change.

## Independent quality approval and limits

The fresh quality reviewer reran HTTP24 and three additional controls: case-insensitive literal and credential header collisions reject before traffic; deeply nested JSON fails with uncertainty and no write while a valid control still writes; compressed allow bytes do not become an approval. All three passed. The reviewer verified all five source hashes and all 49 original/correction artifacts. `/tmp/plugin-http-quality-review.md` records READY with no finding requiring correction; the extra controls are in `/tmp/plugin-http-quality-controls.rs` and their output in `/tmp/plugin-http-quality-controls.log`.

The reviewer assessed every static finding category. Repeated private fixture setup is real but keeps independent integration tests isolated; a shared fixture framework would add coupling. LimitedInput moved unchanged from command.rs so both runners share one encoder. Similar finite defaults do not justify merging distinct command and HTTP authority types. The four longer production functions keep sequential boundary checks together; no production complexity regression was established. Test/trait reachability and the Peer name collision are graph limitations confirmed against actual uses and executed tests. These judgments do not change the nonzero static exit codes or suppress findings.

The source-validated HTTP result is decoded again by the dispatcher from its original bytes. Credentials from both endpoint bindings are filtered before retention, while each request retains its own headers. Cancellation closes the owned exchange and preserves uncertainty; it cannot promise a remote rollback. Local peers cover HTTP/1.1, while source inspection confirms explicit retry::never. No exhaustive HTTP/2, public activation, remaining lifecycle, live-provider or complete-commitment claim is made. The host must genuinely admit the separate read-only endpoint; no remote compare-and-swap is implemented here.

The parent reviewed the production standard before committing: scope and interfaces are coherent, input/secret/lifetime boundaries have executed controls, the discovered defect is corrected, relevant regressions and required checks ran, and both independent reviews approved. No known defect remains in this bounded prerequisite. The plan retains all remaining work and exactly one active item.
