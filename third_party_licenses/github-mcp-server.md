# GitHub MCP integration

`plugins/github` connects to GitHub's official remote MCP service without
bundling its server, Docker image or another runtime. The endpoint, PAT header
and focused toolsets were reviewed against
[`github/github-mcp-server` at `71ef8266e48110974b13aef50b4df6ff9914ff68`](https://github.com/github/github-mcp-server/tree/71ef8266e48110974b13aef50b4df6ff9914ff68),
specifically `docs/remote-server.md` and
`docs/installation-guides/install-zed.md`. That revision documents that the
hosted server does not advertise OAuth for non-Copilot clients, so this package
uses the supported personal-access-token header path.

The upstream source is MIT licensed, Copyright (c) 2025 GitHub. Its full license
and the adaptation notice ship in `plugins/github/LICENSE.upstream` and
`plugins/github/NOTICE`. Sailry owns the package and workflow skill adaptation.
The reviewed revision is provenance, not a pin of the hosted service.
