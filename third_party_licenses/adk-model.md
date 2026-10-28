# Historical ADK client attribution

Before the Goose GDK migration, Sailry used the model and session libraries
from [dux-web/adk-rust](https://github.com/dux-web/adk-rust), a fork of
[ADK-Rust](https://github.com/zavora-ai/adk-rust), at
[`675c02b6c0df6d632b953a06a5a12d1f814b7780`](https://github.com/dux-web/adk-rust/commit/675c02b6c0df6d632b953a06a5a12d1f814b7780).
These libraries are no longer runtime dependencies. Retained browser, sandbox,
MCP and test-support adaptations carry source attribution at their owning files;
see also [adk-support.md](adk-support.md).

ADK's Apache-2.0 copyright notice remains in [adk-LICENSE](adk-LICENSE).
The former Gemini client's original MIT notice, Copyright (c) 2025 Michael Yang,
remains in [adk-gemini-LICENSE](adk-gemini-LICENSE).
