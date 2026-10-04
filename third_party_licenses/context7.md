# Context7 integration

`plugins/context7` connects to the official Context7 remote MCP service without
bundling its server or requiring a JavaScript runtime. The endpoint, API-key
header and lookup workflow were reviewed against
[`upstash/context7` at `bfa02ea67b5707fe0e0a673faa49d0f50b28c80b`](https://github.com/upstash/context7/tree/bfa02ea67b5707fe0e0a673faa49d0f50b28c80b),
specifically `packages/mcp/README.md` and `packages/mcp/src/index.ts`.

The upstream source is MIT licensed, Copyright (c) 2021 Upstash, Inc. Its full
license and the adaptation notice ship in `plugins/context7/LICENSE.upstream`
and `plugins/context7/NOTICE`. Sailry owns the package and skill adaptation.
The reviewed revision is provenance, not a pin of the hosted service.
