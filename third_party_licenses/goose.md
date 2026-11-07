# Retained Goose-derived source

Sailry's Node-owned Agent loop, compaction and provider adapters retain source and
semantics adapted from [aaif-goose/goose](https://github.com/aaif-goose/goose)
under Apache-2.0. Goose's `goose-agent`, `goose-provider-types`,
`goose-providers` and `goose-context-management` crates are not runtime
dependencies.

Two source revisions retain distinct provenance:

- Upstream [`ee8500d5e060eea6f2bcee92e8a5e351f12c3a70`](https://github.com/aaif-goose/goose/commit/ee8500d5e060eea6f2bcee92e8a5e351f12c3a70):
  `crates/node-runtime/src/agent/engine/operations.rs`, `steer.rs` and the
  orchestration in `compact.rs` adapt the application state-machine operations.
  `engine/tokens.rs` adapts `crates/goose/src/token_counter.rs` for Node's context
  accounting.
- Sailry's fork [`14741e9a7639810840b43767bd1218a6a43e87ec`](https://github.com/sailry/goose/commit/14741e9a7639810840b43767bd1218a6a43e87ec):
  `engine/mod.rs`, `conversation.rs`, `inference.rs`, `tools.rs`, `foreground.rs`
  and `inference/projection.rs` retain operation ordering, turn boundaries,
  inference recovery, cancellation, child-delivery and request-repair semantics.
  `engine/compact/` retains the `goose-context-management` summarizer, templates,
  formatter, structured-summary parser and related tests. The provider adapters
  in `agent/model/rig/parameters.rs`, `schema.rs`, `cloud.rs`, `anthropic.rs`,
  `thinking.rs`, `responses.rs` and `codec/history.rs` retain Google request
  policy, OpenAI schema conversion, Azure routing, incomplete-tool handling,
  inline-thinking parsing, Responses framing and snapshot precedence, and
  resource projection. `agent/model/context.rs`, `context/catalog.rs` and
  `context/limits.json` retain context discovery, model-name normalization and
  a context-only projection of the provider catalog.

These adaptations use Sailry-owned execution, model and canonical-history types.
Source modification notices remain alongside the adaptations. The earlier
operation and token-counting adaptations keep their original source revision.

The original Apache-2.0 license and Copyright 2024 Block, Inc. notice are
retained in [goose-Apache-2.0.txt](goose-Apache-2.0.txt).
