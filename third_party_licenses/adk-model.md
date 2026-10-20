# ADK model and native clients

ADK dependencies use [dux-web/adk-rust](https://github.com/dux-web/adk-rust),
a fork of [ADK-Rust](https://github.com/zavora-ai/adk-rust), at
[`e15668344ecaf4a3579ca9978b3c7bb35434ef0d`](https://github.com/dux-web/adk-rust/commit/e15668344ecaf4a3579ca9978b3c7bb35434ef0d).
The root Cargo.toml and Cargo.lock retain that exact Git source for sibling ADK
packages. Sailry does not carry the retired request-scoped provider-error hooks;
native ADK errors supply provider classification and diagnostics.

Only `adk-model` is vendored at that revision, with a focused Responses transport
patch. HTTP and WebSocket share request adapters, compatible event normalization
and response projections. The patch adds refreshed handshake headers, bounded
socket lifetimes, continuation and HTTP fallback, while Node owns authorization,
capability observations and retry budgets. The manifest expands upstream
workspace inheritance and aligns WebSocket TLS roots with the generation HTTP
client. Source provenance and modification details are retained in
[vendor/adk-model/SAILRY_PATCHES.md](../vendor/adk-model/SAILRY_PATCHES.md).

ADK's Apache-2.0 copyright notice is retained in [adk-LICENSE](adk-LICENSE)
and in the vendored crate. The Gemini client's original MIT notice,
Copyright (c) 2025 Michael Yang, is retained in
[adk-gemini-LICENSE](adk-gemini-LICENSE).
