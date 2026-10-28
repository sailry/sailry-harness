# ADK Browser

Sailry's retained Chromium service code is adapted from
[dux-web/adk-rust](https://github.com/dux-web/adk-rust), a fork of
[ADK-Rust](https://github.com/zavora-ai/adk-rust). The reviewed source revision and
retained modules are documented in [adk-support.md](adk-support.md). ADK Browser
is no longer a runtime dependency.

The reviewed fork included upstream PR #680's explicit startup, Chrome
configuration and stale-session recovery fixes. Browser operations now use the
retained Node-owned service code.

Copyright 2026 Zavora Technologies Ltd. Licensed under Apache-2.0;
the original notice is retained in [adk-LICENSE](adk-LICENSE).
