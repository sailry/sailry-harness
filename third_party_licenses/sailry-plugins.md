# Plugin package migration sources

The Node plugin module follows reviewed, committed Sailry-owned sources:

- Sailry Platform `727ce0a`, `rust/crates/harbor-agent-plugins/src/manifest.rs`: portable manifest validation and component failure boundaries, adapted in `crates/node-runtime/src/plugins/manifest.rs`
- Sailry Platform `727ce0a`, `rust/crates/harbor-agent-plugins/src/loader.rs`: fixed-location component discovery and bounded loading behavior
- Sailry Platform `727ce0a`, `rust/crates/harbor-agent-plugins/src/harbor.rs` and `model.rs`: exact extension action declarations and resource-bound invocation semantics, adapted to existing requests in `crates/node-runtime/src/store/plugins/actions.rs` without the historical action registry
- Sailry Code `67ae9fa0`, `rust/crates/sailry-code-plugin-host/src/production/storage/packages.rs`: immutable package objects, complete skill resources and retained revisions

These Sailry-owned adaptations are released under [Apache-2.0](../LICENSE) by their copyright owner. No dirty worktree, Flutter renderer or duplicate execution host was imported. Third-party resources retain their original licenses.

The portable contract is [Agent Plugins 1.0.0](https://agent-plugins.org/specification); skill frontmatter follows [Agent Skills](https://agentskills.io/specification). YAML parsing uses the existing locked `serde-saphyr` 0.0.29 dependency, with only deserialization enabled. The old handwritten YAML subset and ADK's incompatible `allowed-tools` array parser are not copied.
