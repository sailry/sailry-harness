# MCP SDK patch notice

Source provenance, licensing and local changes are documented in
[MCP SDK patches](../../third_party_licenses/rmcp.md).

The reqwest transport also exposes typed HTTP 429 and server-error status with Retry-After metadata
without retaining the response body, for the Node search driver's cooldown.
