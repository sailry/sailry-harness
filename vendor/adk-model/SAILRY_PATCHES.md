# Sailry Responses transport patch

Source: dux-web/adk-rust at e15668344ecaf4a3579ca9978b3c7bb35434ef0d
(https://github.com/dux-web/adk-rust/tree/e15668344ecaf4a3579ca9978b3c7bb35434ef0d/adk-model).
The upstream Apache-2.0 license and source notices are retained.

Only this crate is vendored. Its sibling ADK dependencies retain that exact Git
revision; the manifest expands upstream workspace inheritance for portability.

Sailry changes integrate Responses WebSocket and SSE into the same request
adapter and event projection, with bounded connection/read deadlines, refreshed
handshake headers, continuation, and caller-observed transport fallback.
Node continues to own retry budgets, capability memory and request credentials.

The modified upstream files are `Cargo.toml`, `src/openai/mod.rs`,
`src/openai/responses_client.rs`, `src/openai/ws_transport.rs` and
`tests/openai_ws_transport_tests.rs`. Socket unit tests are added in
`src/openai/ws_transport/tests.rs`. The manifest expands workspace metadata and
dependency inheritance, pins the existing WebSocket version to 0.28.0 and uses
WebPKI roots to match the generation HTTP client. Its independent workspace
permits focused upstream tests; Sailry's root lockfile governs application builds.
