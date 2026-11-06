# OpenAI Codex patch format

Sailry's `apply_patch` agent tool accepts the patch format of
[openai/codex](https://github.com/openai/codex) at
[`451787127c0e70247034c33e358536ffd2b89cf8`](https://github.com/openai/codex/commit/451787127c0e70247034c33e358536ffd2b89cf8).

`crates/node-runtime/src/files/patch/parser.rs` adapts that revision's
`codex-rs/apply-patch/src/parser.rs` and `streaming_parser.rs`, following the
grammar in `codex-rs/core/assets/tools/apply_patch.lark`.
`crates/node-runtime/src/files/patch/update.rs` adapts `file_update.rs`,
`text_file.rs` and `seek_sequence.rs` from the same directory.

Sailry's modifications: paths are worktree-relative strings for the confined
file service; the `*** Environment ID:` preamble is not accepted; context and
removed lines match exactly, without Codex's whitespace and Unicode punctuation
fallbacks; a chunk whose lines also match before the next chunk is rejected as
ambiguous; and updates become exact edits for Sailry's change-set engine instead
of direct filesystem writes. Source modification notices remain in both files.

The original Apache-2.0 license is retained in [codex-LICENSE](codex-LICENSE) and
the original Copyright 2025 OpenAI notice in [codex-NOTICE](codex-NOTICE).
