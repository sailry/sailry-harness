# ADK model and native clients

The model, Anthropic and Gemini dependencies come from
[dux-web/adk-rust](https://github.com/dux-web/adk-rust), a fork of
[ADK-Rust](https://github.com/zavora-ai/adk-rust). The root Cargo.toml and Cargo.lock
pin the exact revision shared by all ADK packages.

The fork includes the provider fixes submitted in upstream PR #679. Its
integration branch also preserves the existing Google Search entry point,
grounding part index and Anthropic request-size error extensions used by Sailry.
These extensions are not part of that upstream PR's stable API changes.

Sailry's local `vendor/adk-model` patch recovers finalized Responses output items
when the terminal event omits them, preserving tool calls and reasoning history.
It also maps ChatGPT's HTTP error details without losing the upstream status.
Nested Responses stream errors are normalized for the pinned SDK, shared-usage
failures retain safe static diagnostics, and terminal failures are not replayed.
The Gemini adapter forwards explicitly requested response modalities through
ADK's generation extensions; this supports native image output without replacing
the pinned model client or its authentication implementation.

ADK's Apache-2.0 copyright notice is retained in [adk-LICENSE](adk-LICENSE).
The Gemini client's original MIT notice, Copyright (c) 2025 Michael Yang,
is retained in [adk-gemini-LICENSE](adk-gemini-LICENSE).
