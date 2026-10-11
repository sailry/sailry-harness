# MCP SDK patches

`vendor/rmcp` contains the published `rmcp 3.1.4` package from
[the official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk/tree/4a738b9dd99eaca418b614afa433a0cbdaf8d056/crates/rmcp).
The registry package records revision `4a738b9dd99eaca418b614afa433a0cbdaf8d056`.
The package declares Apache-2.0. The complete upstream licensing transition,
Apache-2.0, MIT and documentation notices from that revision are retained in
`vendor/rmcp/LICENSE`.
Registry cache markers are omitted; trailing whitespace and final newlines are normalized.
The manifest disables upstream's developer Git hook installer in the vendored build.

The local reqwest patch applies the existing SSE message limit to JSON and HTTP
error response bodies before decoding them. It rejects oversized Content-Length
and stops chunked reads as soon as the limit is exceeded. Read failures propagate
instead of being treated as accepted responses. Sailry configures an 8 MiB limit.

The SDK's response reader and SSE limiter are private, so using a separate client
adapter would duplicate parsing and error handling. Session management, request
correlation, authentication headers, SSE parsing and cleanup stay in the SDK.
No protocol version, transport implementation or dependency version changes.
Replace this patch with a pinned upstream revision after the same regressions pass.

The service cancellation patch also cancels incoming request contexts for client
handlers, including elicitation. It uses the SDK's request-token pool, which is
registered before callbacks are spawned, so cancellation cannot race a separate
application callback registry. Existing outgoing client cancellation remains the
fallback when no incoming request matches.

ADK task polling uses the SDK's existing public service APIs to reach the
connection's registered client handler. The SDK's internal input-batch helper
retains its upstream private visibility.

The reqwest transport preserves HTTP 429 and server-error status with bounded Retry-After metadata
in a typed error before reading a response body. Sailry's search owner uses that
metadata to cool down the affected driver. The error contains no response body
or authentication value, and does not change transport retry behavior.

The dynamic registration patch selects `none` when advertised, otherwise
`client_secret_basic`, then `client_secret_post`, matching the SDK's token
exchange preference. The pinned SDK always requested `none`, preventing
registration with servers advertising only confidential methods. Omitted
authentication-method metadata keeps its existing public-client behavior.
Explicit unsupported methods fail before registration, and confidential
registration requires a returned nonempty client secret. The public API,
dependency version and Node's private credential storage remain unchanged.
