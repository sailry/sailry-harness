# Gemini TLS dependency patch

Source: `adk-gemini` from `dux-web/adk-rust` revision
`ae14f6410e742ff84a73a7b0033a1cb77679b762`.
The committed library source and README are unchanged. The upstream subtree's
MIT `LICENSE` and the ADK workspace's Apache-2.0 `LICENSE-APACHE` are both retained;
the manifest keeps its upstream Apache-2.0 declaration.

The standalone manifest resolves inherited package and dependency fields and
includes the library and its inline tests, not upstream example or external
test targets. Google AI Platform and Google authentication no longer force
their default Rustls provider. Reqwest retains the same native and WebPKI trust
roots, HTTP/2, JSON, streaming and multipart features without selecting a crypto
provider. Sailry installs ring before constructing provider clients.

No Gemini, Vertex, authentication, request or retry implementation is changed.
Remove this patch when the pinned upstream manifest permits the application to
select its Rustls crypto provider.
