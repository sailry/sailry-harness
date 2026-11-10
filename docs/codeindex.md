# Code index

The Node owns the code index in `internal/node/codeindex`; the wiring is
`internal/node/codeindex_service.go` and the settings and index schema are in
`internal/node/store/code_index.go`. There is no UI surface: controllers and the
Agent reach it through commands.

Each worktree has one disposable index at
`<profile>/cache/code-index/<WorktreeId>/index.sqlite`. It is built lazily on the
first query, refreshed before later queries when the worktree watch reported a
change, and deleted when its worktree or project is removed. Parsing uses pure-Go
`github.com/odvcencio/gotreesitter`; coverage is exactly that version's grammar
registry and nothing downloads at runtime.

| Command | Output | Notes |
|---|---|---|
| `search_code` | `code_search` | Full-text and symbol search |
| `outline_code` | `code_outline` | Symbols of one file |
| `graph_code` | `code_graph` | Bounded name-based graph queries without type information |
| `cancel_code_index_query` | `code_index_query_cancelled` | Cancels a running query of the same caller |
| `read_code_index_settings` | `code_index_settings` | Defaults to enabled at revision 0 |
| `save_code_index_settings` | `code_index_settings` | Durable; the service is reconfigured before the command completes |

Queries are registered per caller and request, at most 128 at a time, with a
120-second deadline. A disabled index answers `NotConfigured`; a duplicate or
excess query and an expired deadline answer `Busy`; a cancelled query answers
`Cancelled`. The operations `code.search`, `code.outline` and `code.graph` map
Agent tools onto the same commands; the bundled code package exposes
`code_search` and `code_outline` directly and builds `code_context`,
`code_impact` and `code_map` on `code.graph`. Results given to the model use
one-based lines. An index build holds the keep-awake request (see
[settings](settings.md)).

## Deviations

| Deviation | Reason |
|---|---|
| The index fingerprint differs from the reference, so each engine rebuilds the other's rows | The parser and extraction rules are a different implementation; the index is disposable |
| Full-text ranking uses SQLite FTS5 `bm25` | The reference ranks with its own database's score function; the ordering rules are the same, scores can differ |
| A change marks the whole worktree stale and the next query rescans by revision | The reference invalidates single paths at each write site; the watch subscription already covers external and internal edits |
| The index setting is not tied to the code package's enabled state | The reference couples both; here a disabled package only removes its tools |
| `code_search` and `code_outline` tool results keep the `{kind, data}` envelope | Direct operations return the command output; the reference returns the shaped data alone |
| The prune deadline is checked between batches of 64 files | Checking per file adds nothing at that batch size |
| SQL, just and caddy grammars come from the registry's sources, Go tags fall back per pattern, and seven injection queries the engine rejects are skipped | Coverage follows the pinned parser version |
| Page-rank scores can differ from the reference in the last floating-point digit | Fused multiply-add differs between the toolchains; results stay deterministic |
