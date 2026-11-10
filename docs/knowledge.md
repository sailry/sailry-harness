# Knowledge

Knowledge indexes folders, files and worktree paths on their Node and answers
searches with cited, revision-checked sources. The reference is
`crates/node-runtime/src/knowledge` and `store/knowledge`.

## Commands

| Command | Behavior |
|---|---|
| `list_knowledge_collections` | The collections the request may read, with revisions and progress |
| `create_knowledge_collection`, `update_knowledge_collection` | Validate the definition and its source; an update that changes what is indexed advances the generation |
| `delete_knowledge_collection` | Removes the collection and its derived index; sources are kept |
| `rebuild_knowledge_collection` | Advances the rebuild counter; every source is read again |
| `pause_knowledge_collection` | Sets or clears the pause; a paused collection keeps its index and is skipped |
| `query_knowledge` | Keyword, semantic or hybrid search over readable collections |
| `read_knowledge_citation` | A page of the cited source after rechecking its revision |

Mutations are durable: they are admitted, executed in one savepoint with the
plugin authority check (`knowledge.read` for the reads, `knowledge.write` for
the rest), recorded with a `knowledge_collections_changed` event and replayed
by request ID. A successful mutation invalidates the running pass.

## Indexing

`internal/node/knowledge/service` runs one worker per Node. A pass reads the
catalog and, per collection that is not paused, binds its generation, model
and scope, scans the source and replaces what changed.

- Sources: a worktree path, or a controller directory or file. Scans descend
  at most 64 levels through `os.Root`, skip symlinks, protected names and
  `.gitignore` matches, and apply the collection's globs. A source is at most
  32 MiB.
- Text: Office files, PDFs and images go through the document inspection used
  by `read_office`, with OCR on the scheduler's background lane; other files
  are decoded as text into one `lines` part. A source whose size and
  modification time are unchanged is not read again; an image without an OCR
  model is skipped as excluded.
- Chunks: `knowledge.Split` over model token offsets when a model is bound,
  otherwise over characters. Every chunk is keyword-indexed in the knowledge
  database's FTS table.
- Vectors: the collection's own model, or the enabled default embedding
  choice, is bound when the registry knows its dimension. Vectors are written
  only while that model is installed, in batches of four on the background
  lane; otherwise the collection is keyword only and its progress has no
  model. Nothing is downloaded and no other model is substituted.
- Scheduling: a pass runs on start, after a mutation, a watch event (250 ms
  quiet, 1 s at most) and every five minutes while a watch backend failed. A
  collection whose pass fails is reported paused and logged; the others
  continue. Progress is published at most every 500 ms and at the end.

Reactions (`reactKnowledge`): a changed embedding choice or installed
embedding set invalidates the pass, and saving the choice advances the
generation of collections without their own model so their vectors are
cleared and rebuilt; a changed OCR choice or installed OCR set refreshes.

## Search

`query_knowledge` takes the top 100 keyword rows (`bm25` with column weights
2, 1, 2, 1) and the top 100 cosine rows of the query vector,
encoded on the interactive lane. Hybrid fuses both by reciprocal rank;
semantic and hybrid fall back to keyword when no collection has vectors for
the model. Limits default to 5 and are bounded at 100; hits are marked
untrusted, and hits of a collection that is still indexing are marked stale.

## Citations

`read_knowledge_citation` checks that the request may read the collection,
finds the chunk, reads the source again and compares its revision. It reports
`RevisionConflict` "knowledge collection changed; refresh its results" when
the collection's definition moved, `NotFound` "knowledge citation is no longer
indexed" for a missing chunk and `RevisionConflict` "knowledge source changed;
refresh its results" for a stale revision. `context` selects the whole source
instead of the chunk; pages are bounded at 64 KiB with `next_offset`.

## Agent tools

`knowledge.query`, `knowledge.read`, `knowledge.collections` (read-only,
concurrent) and `knowledge.create`, `knowledge.rebuild`, `knowledge.remove`
(approved) are declared in `agent/operations/specs.json` with the reference's
descriptions and schemas and bound in `knowledge_service_tools.go`. A call
runs as a request of the declaring package in the turn's worktree and
conversation. `knowledge.query` clamps `limit` to 100 and `knowledge.read` to
64 KiB with `context` defaulting to true; `knowledge.create` resolves a
worktree source to the captured worktree. A tool appears only when an enabled
package declares its operation.

## Desktop

The shell's knowledge viewer and its live wiring are specified in
[shell.md](shell.md#knowledge).

## Ownership

| Package | Owns |
|---|---|
| `internal/node/knowledge` | Pure helpers: validation, globs, chunking, ranking, vectors |
| `internal/node/knowledge/source` | Scanning and reading sources |
| `internal/node/knowledge/service` | Index, search, citations, worker and watches |
| `internal/node/store/knowledge*.go` | Collections, grants and progress in the Node database |
| `internal/node/knowledge_service*.go` | Dispatch, model and OCR adapters, reactions, agent tools |

## Tests

`internal/node/knowledge/service/service_test.go`: `TestKnowledgeQuery`,
`TestCitationRecheck`, `TestCollectionLifecycle`, with a fake embedding
function at the capability seam and an injected clock.
`internal/node/knowledge_service_integration_test.go` (`integration`):
`TestKnowledgeIndexing` indexes text, an image and a scanned PDF with the
pinned embedding and OCR models, searches, reads a citation and re-indexes
after the embedding choice is turned off.

## Deviations

| Deviation | Reason |
|---|---|
| Plugin storage sources are rejected as not configured; automatic semantic collections and the refresh on plugin commands and storage changes are not ported | They depend on plugin storage indexing outside this scope |
| Keyword scores come from SQLite FTS5 `bm25` | The Go store is SQLite; the reference scores with its own engine's FTS |
| Globs use the package's translator; chunk keys and scope hashes hash Go's JSON encoding | No byte compatibility with a reference index is required; indexes are derived data |
| Directory entries are scanned in sorted order | Deterministic passes and tests |
| Watches use fsnotify per directory | The module's existing watcher; a failed backend falls back to the five-minute poll |
| The OCR extractor signature is the hash of the selected OCR resource | It changes exactly when recognized text may change |
| Durable knowledge mutations complete inside `Dispatch` | They are single short transactions |
| Citations open in the Desktop viewer only | See [shell.md](shell.md#deviations) |
