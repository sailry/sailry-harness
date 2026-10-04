# Sailry Code terminal source

Adapted from the committed Sailry Code revision `67ae9fa0`:

- `rust/crates/sailry-protocol/src/terminal_workspace.rs`: semantic input and screen contracts
- `rust/crates/sailry-terminal-host/src/vt.rs`: Ghostty projection, input encoding, and tests
- `rust/crates/sailry-terminal-host/src/session.rs`: Unix cancelable PTY I/O algorithm
- `rust/crates/sailry-terminal-host/src/shell.rs`: account login environment initialization
- `rust/crates/sailry-terminal-activity/src/lib.rs`: OSC activity reduction
- `app/sailry_code_app/lib/terminal/terminal_natural_text_editing.dart`: macOS editing shortcut semantics

These Sailry-owned adaptations are released under [Apache-2.0](../LICENSE) by their copyright owner. The adapted sections carry no additional file-level copyright notices. This adaptation changes module ownership, public names, error handling, and Node integration; it does not import the former Host or Flutter implementation.

Ghostty and libghostty-rs retain their pinned revisions and upstream licenses. The raw binding crate is vendored for a focused native prompt-resize patch; see `ghostty-terminal.md`.
