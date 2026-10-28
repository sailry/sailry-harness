# ADK Agent

Sailry previously used the Agent runtime from
[dux-web/adk-rust](https://github.com/dux-web/adk-rust), a fork of
[ADK-Rust](https://github.com/zavora-ai/adk-rust). This file records that historical
provenance; ADK Agent packages are no longer runtime dependencies. Retained
support code is documented in [adk-support.md](adk-support.md).

The reviewed fork included the response, citation, continuation and tool-lifecycle
fixes submitted in upstream PR #678. Its regression suite included the citation
and continuation tests previously referenced from Sailry's vendor directory.

Copyright 2026 Zavora Technologies Ltd. Licensed under Apache-2.0;
the original notice is retained in [adk-LICENSE](adk-LICENSE).
