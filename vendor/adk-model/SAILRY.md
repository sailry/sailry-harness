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

Selected TLS dependencies defer the process crypto provider to the application
instead of forcing AWS-LC through Reqwest or async-openai. No request, retry or
conversion behavior changes. Sailry does not enable the upstream Bedrock or
Vertex SDK features.

Responses transport uses the request's streaming flag when framing successful
SSE bodies, including Codex responses without a Content-Type header. Normalization
preserves incremental delivery with LF, CRLF and CR line endings across split
HTTP chunks. Explicit nonstreaming requests retain JSON response normalization.
Remove this patch when the pinned upstream transport preserves these contracts.
