# Ghostty terminal integration

The Unicode placeholder decoding rules and diacritic table are adapted from Ghostty `ab0b9da9e88fcb4b0533a1854e84628f663930af`, `src/terminal/kitty/graphics_unicode.zig`. The MIT license is preserved in `ghostty-MIT.txt`.

The shell integration scripts are imported from the same revision under `src/shell-integration/{zsh,bash,fish}`. Their original license headers are retained. Sailry adds explicit `redraw=1` to Zsh and Fish OSC 133 prompt markers because the embedded library disables prompt redraw by default. Zsh and Bash integration include GPL-3.0-or-later material; see [the GPL text](sailry-code-GPL-3.0.txt). Despite that license file's historical name, it is retained for this third-party material, not as the license for Sailry-owned adaptations. The bundled bash-preexec retains its MIT notice.

The Rust wrapper and raw bindings use `sailry/libghostty-rs` at `0b239afc87748c2a4bced30194a87f075a474a4b`, based on upstream `Uzaaft/libghostty-rs` at `d2036baeb605d31d09a9e2ba6d0c923e04c34d54`. The fork only selects the GNU libc 2.28 build target for native and cross Linux builds. The wrapper's MIT notice is retained in `libghostty-rs-MIT.txt`.

The wrapper builds unmodified native Ghostty `3425025e585a3403340d4dc6d65132f42e05e605` from `ghostty-org/ghostty`. Sailry does not apply title-stack or prompt-resize patches to that source.
