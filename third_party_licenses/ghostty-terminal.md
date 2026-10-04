# Ghostty terminal integration

The Unicode placeholder decoding rules and diacritic table are adapted from Ghostty `ab0b9da9e88fcb4b0533a1854e84628f663930af`, `src/terminal/kitty/graphics_unicode.zig`. The MIT license is preserved in `ghostty-MIT.txt`.

The shell integration scripts are imported from the same revision under `src/shell-integration/{zsh,bash,fish}`. Their original license headers are retained. Sailry adds explicit `redraw=1` to Zsh and Fish OSC 133 prompt markers because the embedded library disables prompt redraw by default. Zsh and Bash integration include GPL-3.0-or-later material; see [the GPL text](sailry-code-GPL-3.0.txt). Despite that license file's historical name, it is retained for this third-party material, not as the license for Sailry-owned adaptations. The bundled bash-preexec retains its MIT notice.

The libghostty-vt-sys binding crate is vendored from libghostty-rs `6111c4d72f11f0a1894cf3c5943b9ee25a6d0c61`; its license is retained under `vendor/libghostty-vt-sys/LICENSE`. The build applies the focused native prompt-boundary correction recorded in `vendor/libghostty-vt-sys/SAILRY.md`, without changing the pinned Ghostty revision.
