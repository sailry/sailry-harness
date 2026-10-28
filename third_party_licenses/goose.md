# Goose GDK

Sailry uses the four GDK libraries `goose-agent`, `goose-provider-types`,
`goose-providers` and `goose-context-management` from
[aaif-goose/goose](https://github.com/aaif-goose/goose) at
[`ee8500d5e060eea6f2bcee92e8a5e351f12c3a70`](https://github.com/aaif-goose/goose/commit/ee8500d5e060eea6f2bcee92e8a5e351f12c3a70).
The root workspace selects the upstream `main` branch; Cargo.lock captures the
exact revision. The application does not depend on the main `goose` crate or
local inference.

`crates/node-runtime/src/agent/engine/tokens.rs` adapts the token-counting
semantics from the same revision's `crates/goose/src/token_counter.rs` for
Node's context accounting. Source modification notices remain alongside the
adaptation.

The original Apache-2.0 license and Copyright 2024 Block, Inc. notice are
retained in [goose-Apache-2.0.txt](goose-Apache-2.0.txt).
