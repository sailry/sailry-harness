# Provider authorization references

Sailry's Node-owned device authorization adapters use the following reviewed protocol references:

- Sailry Code `67ae9fa0`, `rust/crates/sailry-code-agent/src/oauth_accounts.rs` and `rust/crates/sailry-host-runtime/src/code_runtime/ai_oauth.rs`: explicit device prompts, bounded concurrent attempts, cancellation, and revocation races. The old Rig runner and credential/configuration caches are not imported.
- [OpenAI Codex `ed6dde9fda2a254a6a2103b1f2753f6825d3acdf`](https://github.com/openai/codex/blob/ed6dde9fda2a254a6a2103b1f2753f6825d3acdf/codex-rs/login/src/device_code_auth.rs): ChatGPT device authorization endpoints, polling responses, public client identifier, and authorization-code exchange. This is a protocol reference; no Codex source files or executable are included.
- [rig-core 0.42.0](https://docs.rs/crate/rig-core/0.42.0/source/src/providers/): ChatGPT and Copilot device authorization, token refresh, inference headers/request fields, token metadata, and Copilot service endpoint restrictions. Adapted behavior lives in `crates/node-runtime/src/providers/login/` and `crates/node-runtime/src/agent/model/authorization.rs`; the applicable MIT notice is preserved below. Rig is not a runtime dependency.
- [GitHub Copilot authentication documentation](https://docs.github.com/en/copilot/how-tos/copilot-sdk/auth/authenticate) and [OpenAI authentication documentation](https://developers.openai.com/codex/auth/): account authorization context. Copilot's internal token endpoint is a vendor integration interface, not the public Copilot administration REST API; fixture tests cannot establish live account availability or service approval.
- [OpenAI Codex model endpoint](https://github.com/openai/codex/blob/a956835d020762cb2b570053af06f643a11c0ecc/codex-rs/codex-api/src/endpoint/models.rs) and [model metadata](https://github.com/openai/codex/blob/a956835d020762cb2b570053af06f643a11c0ecc/codex-rs/protocol/src/openai_models.rs): account model discovery, client-version query, input modalities and native reasoning choices. The reviewed catalog compatibility baseline is `0.160.0`, from [Codex's released workspace version](https://github.com/openai/codex/blob/a956835d020762cb2b570053af06f643a11c0ecc/codex-rs/Cargo.toml); this is distinct from Sailry's package version and preserves Sailry's own request identity. Missing limits remain unknown; no upstream default catalog or legacy decoder is imported.
- [VS Code Copilot Chat `5863f5a7088958050792b5dccbe8b46c6e13eccc`](https://github.com/microsoft/vscode-copilot-chat/blob/5863f5a7088958050792b5dccbe8b46c6e13eccc/src/platform/endpoint/common/endpointProvider.ts) and [endpoint selection](https://github.com/microsoft/vscode-copilot-chat/blob/5863f5a7088958050792b5dccbe8b46c6e13eccc/src/platform/endpoint/node/chatEndpoint.ts): native model types, API surfaces, context/output limits and optional capabilities. These are protocol references; no TypeScript source is included. The rig-core model lister also informed request headers.

The public OAuth client identifiers are application identifiers, not client secrets. Production authorization destinations are fixed. Only the compile-time test-support bootstrap can replace HTTP destinations. Access/renewal grants belong to the existing protected Node credential store; authorization codes are transient and no separate auth cache, CLI process, or Agent engine is introduced.

Device login, protected persistence, shared observation, refresh and ADK text/tool
generation are verified using isolated HTTP services. ChatGPT uses streamed,
stateless Responses and omits fields rejected by that endpoint; Copilot selects
the explicitly configured API and preserves user/agent initiation. Refresh work
belongs to Node, so cancelling an observing turn does not discard a rotated token.
A persisted marker prevents replay of an uncertain ChatGPT rotation. The Copilot
GET exchange does not rotate its GitHub grant and can be requested by a later turn.
ChatGPT grants are not copied between execution Nodes; each execution Node needs
its own login. The minimal Mobile bridge consumes the shared login projection;
Dart/iroh fixtures cover explicit cancellation, observer release, controller
restart and authorized generation. Desktop Kit dialogs use the same Node commands
and Client observation; local/iroh fixtures cover cancellation, delayed admission,
receipt recovery, revocation and reconnect. Account model queries use the same
Node-owned grant resolution and refresh, without adding another catalog cache or
sending grants to controllers. Successful login fills an empty model selection in
the existing Provider configuration; reconnect preserves an existing selection.
Copilot discovery filters the configured generation API;
unknown native reasoning values are not converted to another level. Desktop and
Mobile consume the shared metadata, and Desktop preserves drafts and unknown
limits. Cross-Node account selection and live account acceptance remain separate
work.

## rig-core MIT license

Copyright (c) 2024, Playgrounds Analytics Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
