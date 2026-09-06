# Independent adapter registration check

`tests/registry_driver.rs` implements a fixture Model in a separate test
crate and registers its factory through the public versioned Registry.
The normal configuration loader selects a named profile for that adapter.
The unchanged native loop requests a real workspace read, returns its
result to the fixture model, and sends its response through the production
terminal. The Python driver supplies unpredictable prompt and file values
and checks their rendered response and retained events.

Omitting registration makes the same driver fail before the prompt with
`unknown adapter`. Restoring registration passes. The first driver draft
occasionally selected the application binary from Cargo's artifact stream;
selecting the named test target explicitly fixed that test-driver defect.
Three consecutive startup runs and the restored failure demonstration passed.

No provider-specific loop or renderer change was needed. This proves the
versioned Rust registration boundary. It does not establish a dynamic
extension loader or separately installed executable adapters.
