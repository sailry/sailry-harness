# ADK model and native clients

The model, Anthropic and Gemini dependencies come from
[dux-web/adk-rust](https://github.com/dux-web/adk-rust), a fork of
[ADK-Rust](https://github.com/zavora-ai/adk-rust). The root Cargo.toml and Cargo.lock
pin the exact revision shared by all ADK packages.

The fork includes the provider fixes submitted in upstream PR #679. Its
integration branch also preserves the existing Google Search entry point,
grounding part index and Anthropic request-size error extensions used by Sailry.
These extensions are not part of that upstream PR's stable API changes.

ADK's Apache-2.0 copyright notice is retained in [adk-LICENSE](adk-LICENSE).
The Gemini client's original MIT notice, Copyright (c) 2025 Michael Yang,
is retained in [adk-gemini-LICENSE](adk-gemini-LICENSE).
