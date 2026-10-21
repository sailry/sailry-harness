# ADK model and native clients

ADK dependencies use [dux-web/adk-rust](https://github.com/dux-web/adk-rust),
a fork of [ADK-Rust](https://github.com/zavora-ai/adk-rust), at
[`ef4f60c6fdefbc9dd607c6e13b51e6fe1dcd45c5`](https://github.com/dux-web/adk-rust/commit/ef4f60c6fdefbc9dd607c6e13b51e6fe1dcd45c5),
based on upstream
[`caac72bef1fd570fd42d3b289410c012fe2df7ce`](https://github.com/zavora-ai/adk-rust/commit/caac72bef1fd570fd42d3b289410c012fe2df7ce).
The root Cargo.toml and Cargo.lock retain that exact Git source for every ADK
package. Sailry does not carry the retired request-scoped provider-error hooks;
native ADK errors supply provider classification and diagnostics.

The fork carries explicit OpenCode transport timeouts from
[`f09867b9a914fad316a429cef138af98f5ae13ea`](https://github.com/dux-web/adk-rust/commit/f09867b9a914fad316a429cef138af98f5ae13ea).
It preserves provider replay parts between invocations and shares Responses
request adapters, compatible event normalization and response projections across
HTTP and WebSocket. Node owns authorization, capability observations and retry
budgets. ADK source is consumed directly from the fork without a local override.

ADK's Apache-2.0 copyright notice is retained in [adk-LICENSE](adk-LICENSE).
The Gemini client's original MIT notice,
Copyright (c) 2025 Michael Yang, is retained in
[adk-gemini-LICENSE](adk-gemini-LICENSE).
