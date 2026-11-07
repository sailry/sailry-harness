# Rig provider transport

Sailry uses `rig-core` **0.46.0** from
[0xPlaygrounds/rig](https://github.com/0xPlaygrounds/rig), published from
[`6e039cd24994bc1e7d963af6f6278df73a81b130`](https://github.com/0xPlaygrounds/rig/commit/6e039cd24994bc1e7d963af6f6278df73a81b130),
path `crates/rig-core`, under the MIT license. The root Cargo workspace pins
`=0.46.0` with default features disabled and the `rustls` feature enabled.
Cargo.lock records the registry archive checksum
`c46316e31b3e80a050ad35790f1b612b4dfdb46982293cc78641caf4271f1b05`.

Rig provides native model request encoding and HTTP/SSE response decoding behind
`crates/node-runtime/src/agent/model/rig`. Its types remain inside that adapter.
Sailry owns the Agent loop, tools, canonical history, compaction, request recovery,
authorization and credential storage. Sailry does not use `rig-agent` or a Rig
Agent runner.

Earlier authorization adaptations reference `rig-core` 0.42.0 separately in
[provider-authorization.md](provider-authorization.md). Their original provenance
remains unchanged by the provider transport dependency.

The original MIT license and Copyright 2024 Playgrounds Analytics Inc. notice are
retained in [rig-MIT.txt](rig-MIT.txt).
