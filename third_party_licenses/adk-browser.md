# ADK Browser

The browser dependency comes from [dux-web/adk-rust](https://github.com/dux-web/adk-rust),
a fork of [ADK-Rust](https://github.com/zavora-ai/adk-rust). The root Cargo.toml and
Cargo.lock pin the exact revision shared by all ADK packages.

The fork includes upstream PR #680's explicit startup, Chrome configuration and
stale-session recovery fixes. Browser operations remain ADK implementations.

Copyright 2026 Zavora Technologies Ltd. Licensed under Apache-2.0;
the original notice is retained in [adk-LICENSE](adk-LICENSE).
