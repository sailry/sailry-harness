# MCP SDK patches

Sailry uses `rmcp 3.5.1` from the pinned
[sailry/rust-sdk fork](https://github.com/sailry/rust-sdk/tree/e12063473f6e629b7a2b066ab3a9cfaf8ba21ccf/crates/rmcp),
based on [the official Rust SDK v3.5.1](https://github.com/modelcontextprotocol/rust-sdk/tree/79437f291b2c44053d00dcd5db969fd0cca7c887/crates/rmcp).
The root Cargo.toml applies the fork across all crates.io `rmcp` dependencies,
including ADK's MCP feature. The upstream Apache-2.0, MIT and documentation
licensing-transition text is retained in [rmcp-LICENSE](rmcp-LICENSE).
The fork's `rmcp-macros 3.5.1` workspace package uses the same license text.

The fork's reqwest patch applies the existing SSE message limit to JSON and HTTP
error response bodies before decoding them. It rejects oversized Content-Length
and stops chunked reads as soon as the limit is exceeded. Read failures propagate
instead of being treated as accepted responses. Sailry configures an 8 MiB limit.

The SDK's response reader and SSE limiter are private, so using a separate client
adapter would duplicate parsing and error handling. Session management, request
correlation, authentication headers, SSE parsing and cleanup stay in the SDK.
These patches add no new protocol version or alternate transport implementation.
The fork remains pinned until an upstream revision passes the same regressions.

The fork's service cancellation patch also cancels incoming request contexts for client
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
exchange preference. The upstream SDK requested `none`, preventing
registration with servers advertising only confidential methods. Omitted
authentication-method metadata keeps its existing public-client behavior.
Explicit unsupported methods fail before registration, and confidential
registration requires a returned nonempty client secret. The public API and
Node's private credential storage remain unchanged.

Refresh rejection uses the actual token response status before OAuth error decoding.
HTTP 401, explicit `invalid_grant` or `invalid_client` responses, and stored
tokens without refresh credentials notify the existing credential store under
its refresh guard. Missing authorization or a different client does not invalidate
unattempted credentials. Server errors, network failures, timeouts and
cancellation preserve credentials. This adds no public API or credential owner.
