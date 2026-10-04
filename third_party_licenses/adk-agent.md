# ADK Agent

The Agent dependency comes from [dux-web/adk-rust](https://github.com/dux-web/adk-rust),
a fork of [ADK-Rust](https://github.com/zavora-ai/adk-rust). The root Cargo.toml and
Cargo.lock pin the exact revision shared by all ADK packages.

The fork includes the response, citation, continuation and tool-lifecycle fixes
submitted in upstream PR #678. Its own regression suite includes the citation
and continuation tests previously referenced from Sailry's vendor directory.

Copyright 2026 Zavora Technologies Ltd. Licensed under Apache-2.0;
the original notice is retained in [adk-LICENSE](adk-LICENSE).
