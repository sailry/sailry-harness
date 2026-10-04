# ADK tools

The tool dependency comes from [dux-web/adk-rust](https://github.com/dux-web/adk-rust),
a fork of [ADK-Rust](https://github.com/zavora-ai/adk-rust). The root Cargo.toml and
Cargo.lock pin the exact revision shared by all ADK packages.

The fork includes upstream PR #681's direct-call task lifecycle, custom input
handlers, concurrent input batches and resource subscription recovery. The rmcp
transport patches remain separately maintained in vendor/rmcp.

Copyright 2026 Zavora Technologies Ltd. Licensed under Apache-2.0;
the original notice is retained in [adk-LICENSE](adk-LICENSE).
