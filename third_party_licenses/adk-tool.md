# ADK tools

Sailry previously used the tool packages from
[dux-web/adk-rust](https://github.com/dux-web/adk-rust), a fork of
[ADK-Rust](https://github.com/zavora-ai/adk-rust). This file records that historical
provenance; ADK tool packages are no longer runtime dependencies. Retained
support code is documented in [adk-support.md](adk-support.md).

The reviewed fork included upstream PR #681's direct-call task lifecycle, custom
input handlers, concurrent input batches and resource subscription recovery. The rmcp
transport patches remain separately maintained in the pinned rmcp fork described
in [rmcp.md](rmcp.md).

Copyright 2026 Zavora Technologies Ltd. Licensed under Apache-2.0;
the original notice is retained in [adk-LICENSE](adk-LICENSE).
