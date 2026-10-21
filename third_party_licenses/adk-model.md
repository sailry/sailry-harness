# ADK model and native clients

ADK dependencies use [dux-web/adk-rust](https://github.com/dux-web/adk-rust),
a fork of [ADK-Rust](https://github.com/zavora-ai/adk-rust), at
[`af1c17a1d9e683fb5b49946d0ed094954539ebd2`](https://github.com/dux-web/adk-rust/commit/af1c17a1d9e683fb5b49946d0ed094954539ebd2),
based on upstream
[`8805e36fc51c90fa8da27b65c806899a8cf38b13`](https://github.com/zavora-ai/adk-rust/commit/8805e36fc51c90fa8da27b65c806899a8cf38b13).
The root Cargo.toml and Cargo.lock retain that exact Git source for every ADK
package. Sailry does not carry the retired request-scoped provider-error hooks;
native ADK errors supply provider classification and diagnostics.

The fork carries explicit OpenCode transport timeouts from
[`f09867b9a914fad316a429cef138af98f5ae13ea`](https://github.com/dux-web/adk-rust/commit/f09867b9a914fad316a429cef138af98f5ae13ea).
It preserves provider replay parts between invocations and shares Responses
request adapters, compatible event normalization and response projections across
HTTP and WebSocket. Node owns authorization, capability observations and retry
budgets. ADK source is consumed directly from the fork without a local override.
Responses WebSocket connections use the same platform certificate verifier and
Rustls crypto provider as HTTP. Certificate rejection becomes a remembered
WebSocket capability failure only after the corresponding HTTP request succeeds.
Adapter headers remain authoritative without restoring constructor credentials.

ADK's Apache-2.0 copyright notice is retained in [adk-LICENSE](adk-LICENSE).
The Gemini client's original MIT notice,
Copyright (c) 2025 Michael Yang, is retained in
[adk-gemini-LICENSE](adk-gemini-LICENSE).
