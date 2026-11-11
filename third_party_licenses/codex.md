# OpenAI Codex patch format

Sailry's `apply_patch` agent tool accepts the patch format of
[openai/codex](https://github.com/openai/codex) at
[`451787127c0e70247034c33e358536ffd2b89cf8`](https://github.com/openai/codex/commit/451787127c0e70247034c33e358536ffd2b89cf8).

The parser and exact matching in `internal/node/files/patch.go` derive from that
revision's `codex-rs/apply-patch/src/` (`parser.rs`, `streaming_parser.rs`,
`file_update.rs`, `text_file.rs` and `seek_sequence.rs`), following the grammar
in `codex-rs/core/assets/tools/apply_patch.lark`. The adaptation reached this
repository through Sailry's earlier Rust implementation; the file carries the
source notice.

Sailry's modifications: paths are worktree-relative strings for the confined
file service, and updates become exact edits for Sailry's change-set engine
instead of direct filesystem writes.

The original Apache-2.0 license is retained in [codex-LICENSE](codex-LICENSE) and
the original Copyright 2025 OpenAI notice in [codex-NOTICE](codex-NOTICE).
