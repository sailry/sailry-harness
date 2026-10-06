Vendored from Uzaaft/libghostty-rs 6111c4d72f11f0a1894cf3c5943b9ee25a6d0c61 (libghostty-vt-sys 0.2.1). License notices are preserved.

The native Ghostty revision remains ab0b9da9e88fcb4b0533a1854e84628f663930af. `patches/prompt-resize.patch` tracks the OSC 133 prompt boundary before screen reflow, then clears from that tracked cell after a successful resize. Looking up the prompt only after reflow misses prompt prefixes merged into the preceding output row. No screen contents are mutated before fallible resize work completes. Explicit source overrides are not patched.

`patches/title-stack.patch` connects the embedded stream handler to xterm title save/restore, including indexed slots and reset cleanup. The pinned handler otherwise silently discards these operations.

GNU Linux builds explicitly select the glibc 2.28 target, including native builds, so Zig does not inherit a newer runner libc than the Host distribution linker supports. Other native targets keep host detection. Focused build-script tests cover both Linux architectures and native/cross target selection.

Regression coverage is in the Node terminal VT tests, desktop title propagation tests, and real local/remote zsh resize test. The remaining bindings are unchanged upstream code.
