# Provider adapter patches

Source: `adk-model` from `dux-web/adk-rust` revision
`ae14f6410e742ff84a73a7b0033a1cb77679b762`, under its retained Apache-2.0 license.
The existing provider patches are described in the retained Cargo manifest.

The default Gemini generateContent transport now routes system-role text through
the pinned `adk-gemini` builder's `with_system_instruction` API. It preserves text
and ordering across system messages and parts, joining them with newline
separators instead of discarding them or adding them to the user transcript.
Both blocking and streaming requests use the same builder. User messages, model
history, tool responses and generation options retain their existing conversion.

Remove this patch when the pinned upstream adapter preserves system instructions
through the same request boundary.

Provider adapters preserve bounded, structured provider error codes in
`AdkError.details.metadata["provider_error_code"]`. Gemini uses the actual
`BadResponse.code` or the Vertex SDK's HTTP status and typed `ErrorInfo.reason`,
not provider message text.
This lets Sailry distinguish an unsupported media request from authentication,
quota and server failures without publishing unchecked provider bodies or
replaying uncertain work. Remove these changes when upstream preserves the
same structured error facts.

Anthropic generation accepts a generic `with_request_adapter` hook over the full
serialized `MessageCreateParams` and HTTP headers. The hook uses the pinned
`adk-anthropic` replacement-header APIs, so callers can remove API-key
authentication and supply bearer authorization. With no hook, generation retains
the SDK's existing send and stream paths. The hook retains the configured base URL,
prompt caching and the model's retry policy; nested SDK retries remain disabled.
Because the SDK's `message_headers` helper is private, request preparation matches
its three automatic beta conditions before giving the hook final header control.
Changes must remain valid SDK message parameters; this does not add a raw JSON
transport or provider-specific request policy. Remove this patch when the pinned
upstream model adapter exposes the same request customization boundary.

OpenAI-compatible Chat Completions streaming emits `reasoning_content` or its
existing `reasoning` fallback before handling the same chunk's `finish_reason`.
The pinned adapter previously consumed terminal `stop` and `tool_calls` chunks
before emitting their reasoning. This small ordering fix retains that thinking
once, without changing tool-call attributes, completion state, usage, request
replay policy or the separate Responses transport. Remove this patch when the
pinned upstream compatible adapter preserves compound terminal deltas.

Selected TLS dependencies defer the process crypto provider to the application
instead of forcing AWS-LC through Reqwest or async-openai. No request, retry or
conversion behavior changes. Sailry does not enable the upstream Bedrock or
Vertex SDK features.

Responses transport uses the request's streaming flag when framing successful
SSE bodies, including Codex responses without a Content-Type header. Normalization
preserves incremental delivery with LF, CRLF and CR line endings across split
HTTP chunks. Explicit nonstreaming requests retain JSON response normalization.
Remove this patch when the pinned upstream transport preserves these contracts.

Responses transport normalizes every non-success HTTP response into the native
structured API error before async-openai parses its body. Standard error envelopes
retain their codes; nonstandard, malformed and interrupted bodies retain the HTTP
status without exposing their contents. This preserves authentication, quota and
server-error classification for compatible providers. Remove this patch when the
pinned upstream adapter preserves statuses independently of provider error bodies.
