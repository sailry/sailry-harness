# ADK model and native clients

The model, Anthropic and Gemini dependencies come from
[dux-web/adk-rust](https://github.com/dux-web/adk-rust), a fork of
[ADK-Rust](https://github.com/zavora-ai/adk-rust). The pinned fork revision is
[`469c2b9`](https://github.com/dux-web/adk-rust/commit/469c2b9d8c62000aaec583f60e574a5ab437dc8d),
based on upstream [v2.3.0](https://github.com/zavora-ai/adk-rust/releases/tag/v2.3.0)
at `7dd31e2571e0e50ee5039b3edcec1150c106bcc6`. The root Cargo.toml and
Cargo.lock use that exact fork revision for all ADK packages.

The pinned fork retains Sailry's focused Responses streaming, retryable premature
stream EOF without weakening terminal validation, consumed wire-field
validation, and safe typed parse diagnostics, provider error metadata, and Anthropic request adaptation. Its Gemini adapter
forwards explicitly requested response modalities through generation extensions
and preserves uncached system instructions. The Google Search entry point,
grounding part index and Anthropic request-size error extensions remain available
to Sailry. Its HTTP transports use Sailry's shared Rustls crypto provider. These
changes remain in the attributed ADK source, not local vendor packages.

ADK's Apache-2.0 copyright notice is retained in [adk-LICENSE](adk-LICENSE).
The Gemini client's original MIT notice, Copyright (c) 2025 Michael Yang,
is retained in [adk-gemini-LICENSE](adk-gemini-LICENSE).
