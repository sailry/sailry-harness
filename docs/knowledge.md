# Knowledge

Knowledge indexes folders, files, worktree paths and plugin storage on their
Node and answers searches with cited, revision-checked sources. The reference is
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

- Sources: a worktree path, a controller directory or file, or plugin storage
  ([below](#plugin-storage)). File scans descend
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
- Scheduling: a pass runs on start, after a mutation, a plugin storage
  trigger, a watch event (250 ms quiet, 1 s at most) and every five minutes
  while a watch backend failed. A
  collection whose pass fails is reported paused and logged; the others
  continue. Progress is published at most every 500 ms and at the end.

Reactions (`reactKnowledge`): a changed embedding choice or installed
embedding set invalidates the pass, and saving the choice advances the
generation of collections without their own model so their vectors are
cleared and rebuilt; a changed OCR choice or installed OCR set refreshes.

## Plugin storage

A storage source names a scope (`node` or `conversation`) and a key prefix of
at most 128 bytes in the owning package's private values.

Validation (`validateKnowledge`). The collection needs a plugin owner: a
controller without a selected package is refused with `PermissionDenied`
"knowledge storage sources require a real plugin owner". A plugin request must
declare `storage.read`, and `conversation.read` for the conversation scope
("plugin does not declare the captured storage access"). The conversation
scope captures the request's session ("conversation storage requires a
captured session"), which must exist ("knowledge conversation is
unavailable") in the captured worktree ("conversation storage is outside the
captured worktree"). The node scope captures neither. Globs are refused. Only
the owner, or a package the controller shared the collection with, reads it;
conversation collections are visible in their own conversation only.

Scanning (`store/knowledge_storage.go`, `service/storage.go`). The worker
pages the values under the prefix sixteen keys at a time in key order. Each
value is one source whose locator is its key. Its text is the string value,
or the raw JSON of another value; indexed data with a `source` reads the
string at that key and JSON pointer instead. The title is the value's `title`
or `name`, else the key. Tags are the collection's followed by the index
tags. Text is at most 256 KiB. The source revision hashes the entry, its index
data and the text source, so a pass chunks and embeds only values whose
revision changed, rechecks the value before publishing ("knowledge storage
source changed") and prunes the keys that are gone. Storage sources have no
file watches.

Citations carry the key as their path and a `value` position with the key
and pointer the text was read from. `read_knowledge_citation` captures the
value again, compares the revision and position and returns its whole text.

### Automatic collections

A package that declares a `node` storage collection with
`index.semantic: true` and the `storage.read` action gets a collection
without creating one (`store/knowledge_semantic.go`).

- Created when the declared key or prefix holds its first value whose index
  data has a `source`: at Node start, after a package is installed, enabled or
  reconfigured, and in the transaction of the package's storage write.
- Named `<package> storage`, owned by the package, with a `node` storage
  source over the declared selector and no model, globs, tags or shares. Its
  ID is derived from the package name and selector, so it is stable.
- Reads only values with index data whose `source` points at raw text; the
  title is the first index field.
- Each storage write of the package advances the generation of its automatic
  collections, which reports them pending until the next pass. A newer
  installed package version replaces the captured one.
- Never removed automatically. They do not count toward the limit of 128
  collections. A controller may delete one; the next registration creates it
  again.

### Refresh triggers

`executePlugin` runs `followKnowledgeStorage` in the command's transaction:
a `plugin_changed` event registers automatic collections, and a storage
command of a plugin (`write_plugin_value`, `write_indexed_plugin_value`,
`remove_plugin_value`, the conversation value writes, `continue_turn` and a
`plugin_transaction` containing one) registers and advances that package's
collections. A change adds `knowledge_collections_changed`. After the commit
the same commands, package installs, enabling and settings saves, and any
plugin command that registered a collection call `Refresh`. The pass runs on the worker after the usual quiet interval, so a
storage write never waits for indexing. Nothing is queued durably: the pass at
Node start reads every collection again and indexes what changed while it was
down.

### Plugin value search

`search_plugin_values` goes through `Service.SearchStorage`. The store plans
the search (`PrepareSemanticSearch`): the keyword ranking of up to 4096 keys
with their revisions, the package's automatic collections when the mode is
not `keyword` and the query has text, and the keys whose text is indexed. The
service asks the index for the top 100 semantic candidates of those
collections, with `all` as required tags and `any` as alternatives, and fuses
both rankings by reciprocal rank. A `semantic` search keeps keyword matches
only while a collection is indexing, no vector is available, nothing matched
by meaning, or the key has no indexed text. `MaterializeSemanticSearch` reads
each selected value as it is now; a value, tag set, collection or embedding
choice that moved since the plan answers the keyword page with `indexing`
set and requests a pass. The page reports the mode that produced it.

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
| `internal/node/knowledge/service` | Index, search, citations, worker, watches and plugin value search |
| `internal/node/store/knowledge*.go` | Collections, grants, progress, storage sources and automatic collections in the Node database |
| `internal/node/knowledge_service*.go` | Dispatch, model and OCR adapters, reactions, storage triggers, agent tools |

## Tests

`internal/node/knowledge/service/service_test.go`: `TestKnowledgeQuery`,
`TestCitationRecheck`, `TestCollectionLifecycle`, with a fake embedding
function at the capability seam and an injected clock.
`internal/node/knowledge_service_integration_test.go` (`integration`):
`TestKnowledgeIndexing` indexes text, an image and a scanned PDF with the
pinned embedding and OCR models, searches, reads a citation and re-indexes
after the embedding choice is turned off.
`internal/node/knowledge_storage_test.go`: `TestKnowledgeStorage` runs real
turns in which the bundled memory package's `save_memory`, `search_memory`
and `forget_memory` feed, search and empty its automatic collection, then
covers explicit node and conversation storage collections: rejections,
scoping between packages and conversations, citations, embedding only changed
values, removal and a restart. It replaces
the embedding capabilities through the unexported `Options.knowledge` seam.
`TestScriptSDK` covers the storage draft of `prepareKnowledgeCollection`.

## Deviations

| Deviation | Reason |
|---|---|
| The keyword ranking of a plugin value search matches the terms as phrases or the query text, as `search_plugin_values` already did; the reference searches the query alone when no terms are given | One keyword expression for the keyword and fused paths |
| Storage source revisions hash Go's encoding of the entry and index data | Revisions are compared only with the same Node's index |
| Keyword scores come from SQLite FTS5 `bm25` | The Go store is SQLite; the reference scores with its own engine's FTS |
| Globs use the package's translator; chunk keys and scope hashes hash Go's JSON encoding | No byte compatibility with a reference index is required; indexes are derived data |
| Directory entries are scanned in sorted order | Deterministic passes and tests |
| Watches go through `internal/node/fswatch` ([files.md](files.md#file-watches)): on macOS one FSEvents stream per source root, filtered to the scanned directories; elsewhere one fsnotify watch per scanned directory | The reference watches each scanned directory; fsnotify on macOS would hold a descriptor per file. A failed backend falls back to the five-minute poll |
| The OCR extractor signature is the hash of the selected OCR resource | It changes exactly when recognized text may change |
| Durable knowledge mutations complete inside `Dispatch` | They are single short transactions |
| Citations open in the Desktop viewer only | See [shell.md](shell.md#deviations) |
