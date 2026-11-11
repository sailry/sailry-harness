//go:build integration

package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// knowledgeEvents collects the events a Node publishes.
type knowledgeEvents struct {
	mu       sync.Mutex
	received []protocol.EventEnvelope
}

func knowledgeFollow(t *testing.T, node *Node) *knowledgeEvents {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	events := &knowledgeEvents{}
	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			update, err := subscription.Next(ctx)
			if err != nil {
				return
			}
			if envelope, delivered := update.Data.(protocol.EventEnvelope); delivered && update.Kind == "event" {
				events.mu.Lock()
				events.received = append(events.received, envelope)
				events.mu.Unlock()
			}
		}
	}()
	t.Cleanup(func() {
		cancel()
		subscription.Close()
		<-done
	})
	return events
}

// knowledgeCursor is the cursor of the last event the Node recorded.
func knowledgeCursor(t *testing.T, node *Node) uint64 {
	t.Helper()
	var cursor int64
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		return db.QueryRowContext(t.Context(), "SELECT COALESCE(MAX(cursor),0) FROM events").Scan(&cursor)
	}); err != nil {
		t.Fatal(err)
	}
	return uint64(cursor)
}

// published waits until a subscriber received knowledge_collections_changed
// recorded after the cursor.
func (events *knowledgeEvents) published(t *testing.T, label string, after uint64) {
	t.Helper()
	acceptanceEventually(t, label+" publishes knowledge_collections_changed", func() bool {
		events.mu.Lock()
		defer events.mu.Unlock()
		return slices.ContainsFunc(events.received, func(envelope protocol.EventEnvelope) bool {
			return envelope.Cursor > after && envelope.Event.Kind == "knowledge_collections_changed"
		})
	})
}

func knowledgeList(t *testing.T, node *Node) []protocol.KnowledgeCollection {
	t.Helper()
	return acceptanceDo[[]protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "list_knowledge_collections"})
}

func knowledgeSearch(t *testing.T, node *Node, mode protocol.KnowledgeMode, text string) protocol.KnowledgePage {
	t.Helper()
	return knowledgeCall[protocol.KnowledgePage](t, node, protocol.Command{Kind: "query_knowledge", Data: &protocol.KnowledgeQuery{
		Query: text, Collections: []protocol.KnowledgeID{}, Mode: mode, Tags: []string{}, Limit: 5,
	}})
}

func knowledgeRead(t *testing.T, node *Node, citation protocol.KnowledgeCitation, complete bool) (protocol.KnowledgeCitationPage, *protocol.Fault) {
	t.Helper()
	return acceptanceRequest[protocol.KnowledgeCitationPage](t, node, protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_knowledge_citation", Data: &protocol.KnowledgeCitationRequest{
		Collection: citation.Collection, Chunk: citation.Chunk, Revision: citation.Revision, Context: complete, Limit: 16 * 1024,
	}}), knowledgeWait)
}

// TestKnowledgeAcceptance drives a folder collection through the Node with
// the pinned embedding and OCR models: indexing, cited answers, citation
// reads and their faults, pause, resume, rebuild, update, restart and delete.
//
// Indexing waits are bounded by knowledgeWait (180 s) because a pass loads
// the embedding and OCR models; every other wait is bounded by 15 s.
func TestKnowledgeAcceptance(t *testing.T) {
	library := knowledgeEnvironment(t, "SAILRY_ONNXRUNTIME_DIR")
	embeddingFiles := knowledgeEnvironment(t, "SAILRY_EMBEDDING_MODEL_DIR")
	ocrFiles := knowledgeEnvironment(t, "SAILRY_OCR_MODEL_DIR")
	fixtures := knowledgeEnvironment(t, "SAILRY_LOCALMODELS_FIXTURES")
	if err := onnx.Load(library); err != nil {
		t.Fatal(err)
	}

	folder := filepath.Join(t.TempDir(), "notes")
	if err := os.MkdirAll(filepath.Join(folder, "scans"), 0o700); err != nil {
		t.Fatal(err)
	}
	texts := map[string]string{
		"coffee.md":  "# Espresso machine\n\nRun a descaling cycle every three months: fill the tank with citric acid solution and flush twice with clean water.\n",
		"bicycle.md": "# Bicycle\n\nInflate the tyres to 6 bar and oil the chain after riding in the rain.\n",
		"garden.md":  "# Garden\n\nTomatoes need watering every day and a trellis in June.\n",
	}
	for name, text := range texts {
		if err := os.WriteFile(filepath.Join(folder, name), []byte(text), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	for _, name := range []string{"english.png", "scan.pdf"} {
		data, err := os.ReadFile(filepath.Join(fixtures, "ocr", name))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(folder, "scans", name), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}

	profile := filepath.Join(t.TempDir(), "profile")
	recognition := localmodels.OCR()[0]
	knowledgeInstall(t, &recognition, ocrFiles, filepath.Join(profile, "models", "ocr", localmodels.ID(&recognition)))
	// The packaged library location is not where this test loads it from.
	capabilities := LocalModelCapabilities()
	for _, kind := range []protocol.ModelsKind{protocol.ModelsKindEmbedding, protocol.ModelsKindOcr} {
		capability := capabilities[kind]
		capability.Available = nil
		capabilities[kind] = capability
	}
	start := func() *Node {
		t.Helper()
		environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
		node, err := StartWithOptions(t.Context(), profile, Options{
			Environment: &environment, MCP: fixtureMCPOptions(),
			LocalModels: localmodels.Options{Capabilities: capabilities},
		})
		if err != nil {
			t.Fatal(err)
		}
		return node
	}
	node := start()
	stopped := false
	defer func() {
		if !stopped {
			node.Stop(context.Background())
		}
	}()
	model, err := node.localModels.registry.Definition(localmodels.DefaultEmbedding)
	if err != nil {
		t.Fatal(err)
	}
	knowledgeInstall(t, model, embeddingFiles, filepath.Join(profile, "models", "embedding", localmodels.DefaultEmbedding, model.Source.Revision))
	events := knowledgeFollow(t, node)

	path := func(name string) string { return filepath.Join(folder, filepath.FromSlash(name)) }
	top := func(page protocol.KnowledgePage, name string) protocol.KnowledgeHit {
		t.Helper()
		if len(page.Hits) == 0 || page.Hits[0].Citation.Path != path(name) || !page.Untrusted {
			t.Fatalf("top hit is not %s: %+v", name, page)
		}
		hit := page.Hits[0]
		if hit.Citation.Link != knowledge.CitationURL(hit.Citation) || !strings.HasPrefix(hit.Citation.Link, "sailry-knowledge://"+hit.Citation.Collection.String()+"/") || hit.Stale {
			t.Fatalf("hit citation %+v", hit)
		}
		return hit
	}
	definition := protocol.KnowledgeDefinition{Name: "Notes", Globs: []string{}, Tags: []string{}, SharedWith: []string{},
		Source: protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: folder}}}

	// Create and index.
	cursor := knowledgeCursor(t, node)
	created := acceptanceDo[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}})
	events.published(t, "create", cursor)
	if created.Revision != 1 || created.Owner.Kind != "controller" || created.Paused {
		t.Fatalf("created %+v", created)
	}
	began := time.Now()
	indexed := knowledgeReady(t, node, created.ID, func(protocol.KnowledgeCollection) bool { return true })
	progress := indexed.Progress
	if progress.Model == nil || *progress.Model != localmodels.DefaultEmbedding || progress.Sources != 5 || progress.Completed != 5 || progress.Skipped != 0 || progress.Chunks < 5 {
		t.Fatalf("indexed progress %+v", progress)
	}
	t.Logf("indexed %d sources into %d chunks with %s in %s", progress.Sources, progress.Chunks, *progress.Model, time.Since(began).Round(time.Millisecond))

	// Answers carry citations.
	semantic := knowledgeSearch(t, node, protocol.KnowledgeModeSemantic, "how do I remove limescale from my coffee maker")
	if semantic.Mode != protocol.KnowledgeModeSemantic || semantic.Model == nil || *semantic.Model != localmodels.DefaultEmbedding {
		t.Fatalf("semantic search ran as %+v", semantic)
	}
	coffee := top(semantic, "coffee.md")
	if coffee.Citation.Position.Kind != "lines" || coffee.Title == "" {
		t.Fatalf("coffee hit %+v", coffee)
	}
	image := top(knowledgeSearch(t, node, protocol.KnowledgeModeKeyword, "Invoice"), "scans/english.png")
	var scanned *protocol.KnowledgeHit
	for _, hit := range knowledgeSearch(t, node, protocol.KnowledgeModeHybrid, "scanned page text").Hits {
		if hit.Citation.Path == path("scans/scan.pdf") {
			scanned = &hit
			break
		}
	}
	if scanned == nil {
		t.Fatal("the scanned document has no hit")
	}
	bicycle := top(knowledgeSearch(t, node, protocol.KnowledgeModeKeyword, "tyres"), "bicycle.md")

	// A citation reads its excerpt, and its whole source as context.
	excerpt, failure := knowledgeRead(t, node, coffee.Citation, false)
	if failure != nil || excerpt.Context || !excerpt.Untrusted || excerpt.NextOffset != nil || excerpt.Offset != 0 ||
		excerpt.Citation.Link != coffee.Citation.Link || excerpt.Title != coffee.Title || !strings.Contains(texts["coffee.md"], excerpt.Text) || !strings.Contains(excerpt.Text, "descaling") {
		t.Fatalf("coffee excerpt %+v, fault %+v", excerpt, failure)
	}
	whole, failure := knowledgeRead(t, node, coffee.Citation, true)
	if failure != nil || !whole.Context || whole.Text != texts["coffee.md"] || whole.Worktree != nil {
		t.Fatalf("coffee context %+v, fault %+v", whole, failure)
	}
	page, failure := knowledgeRead(t, node, scanned.Citation, false)
	if failure != nil || strings.TrimSpace(page.Text) == "" || page.Citation.Position.Kind != scanned.Citation.Position.Kind || page.Citation.Path != path("scans/scan.pdf") {
		t.Fatalf("scanned page %+v, fault %+v", page, failure)
	}
	position, _ := json.Marshal(page.Citation.Position)
	t.Logf("the scanned document cites %s and reads %q", position, page.Text)
	recognized, failure := knowledgeRead(t, node, image.Citation, false)
	if failure != nil || !strings.Contains(recognized.Text, "Invoice") {
		t.Fatalf("image citation %+v, fault %+v", recognized, failure)
	}

	// Pause: indexing stops, so a changed or deleted source is reported by
	// the read itself.
	cursor = knowledgeCursor(t, node)
	paused := acceptanceDo[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "pause_knowledge_collection", Data: &protocol.CommandPauseKnowledgeCollectionPayload{ID: created.ID, ExpectedRevision: indexed.Revision, Paused: true}})
	events.published(t, "pause", cursor)
	if !paused.Paused || paused.Progress.Status != protocol.KnowledgeStatusPaused || paused.Revision != indexed.Revision+1 {
		t.Fatalf("paused %+v", paused)
	}
	_, failure = acceptanceCall[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "pause_knowledge_collection", Data: &protocol.CommandPauseKnowledgeCollectionPayload{ID: created.ID, ExpectedRevision: indexed.Revision, Paused: false}})
	if failure == nil || failure.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("a stale revision resumed the collection: %+v", failure)
	}
	if still, failure := knowledgeRead(t, node, coffee.Citation, true); failure != nil || still.Text != texts["coffee.md"] {
		t.Fatalf("a paused collection lost its citation: %+v, fault %+v", still, failure)
	}
	changed := "# Espresso machine\n\nReplace the water filter every two months and wipe the steam wand after each use.\n"
	if err := os.WriteFile(path("coffee.md"), []byte(changed), 0o600); err != nil {
		t.Fatal(err)
	}
	_, failure = knowledgeRead(t, node, coffee.Citation, false)
	acceptanceFault(t, "citation of a changed source", failure, protocol.ErrorCodeRevisionConflict, "knowledge source changed; refresh its results")
	if err := os.Remove(path("bicycle.md")); err != nil {
		t.Fatal(err)
	}
	_, failure = knowledgeRead(t, node, bicycle.Citation, false)
	acceptanceFault(t, "citation of a deleted source", failure, protocol.ErrorCodeRevisionConflict, "knowledge source changed; refresh its results")
	if listed := knowledgeList(t, node); len(listed) != 1 || listed[0].Progress.Status != protocol.KnowledgeStatusPaused {
		t.Fatalf("the paused collection indexed: %+v", listed)
	}

	// Resume: the next pass drops what the sources no longer contain.
	cursor = knowledgeCursor(t, node)
	resumed := acceptanceDo[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "pause_knowledge_collection", Data: &protocol.CommandPauseKnowledgeCollectionPayload{ID: created.ID, ExpectedRevision: paused.Revision, Paused: false}})
	events.published(t, "resume", cursor)
	if resumed.Paused || resumed.Revision != paused.Revision+1 {
		t.Fatalf("resumed %+v", resumed)
	}
	current := knowledgeReady(t, node, created.ID, func(collection protocol.KnowledgeCollection) bool { return collection.Progress.Sources == 4 })
	if current.Progress.Completed != 4 || current.Progress.Model == nil {
		t.Fatalf("progress after the edit and the deletion: %+v", current.Progress)
	}
	_, failure = knowledgeRead(t, node, bicycle.Citation, false)
	acceptanceFault(t, "citation of a source the index dropped", failure, protocol.ErrorCodeNotFound, "knowledge citation is no longer indexed")
	_, failure = knowledgeRead(t, node, coffee.Citation, false)
	if failure == nil || !(failure.Code == protocol.ErrorCodeNotFound && failure.Message == "knowledge citation is no longer indexed" ||
		failure.Code == protocol.ErrorCodeRevisionConflict && failure.Message == "knowledge source changed; refresh its results") {
		t.Fatalf("citation of a reindexed source: %+v", failure)
	}
	t.Logf("the old citation of the edited note reports %s %q after reindexing", failure.Code, failure.Message)
	if stale := knowledgeSearch(t, node, protocol.KnowledgeModeKeyword, "descaling"); len(stale.Hits) != 0 {
		t.Fatalf("the replaced text is still indexed: %+v", stale)
	}
	filter := top(knowledgeSearch(t, node, protocol.KnowledgeModeKeyword, "filter"), "coffee.md")
	if fresh, failure := knowledgeRead(t, node, filter.Citation, true); failure != nil || fresh.Text != changed || filter.Citation.Revision == coffee.Citation.Revision {
		t.Fatalf("fresh citation %+v, fault %+v", fresh, failure)
	}

	// Rebuild.
	cursor = knowledgeCursor(t, node)
	rebuilt := acceptanceDo[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "rebuild_knowledge_collection", Data: &protocol.CommandRebuildKnowledgeCollectionPayload{ID: created.ID, ExpectedRevision: current.Revision}})
	events.published(t, "rebuild", cursor)
	if rebuilt.Rebuild != current.Rebuild+1 || rebuilt.Revision != current.Revision+1 || rebuilt.Progress.Generation != current.Progress.Generation+1 || rebuilt.Progress.Status != protocol.KnowledgeStatusPending {
		t.Fatalf("rebuilt %+v from %+v", rebuilt, current)
	}
	current = knowledgeReady(t, node, created.ID, func(collection protocol.KnowledgeCollection) bool {
		return collection.Progress.Generation == rebuilt.Progress.Generation
	})
	if current.Progress.Sources != 4 || current.Progress.Completed != 4 || current.Progress.Chunks == 0 {
		t.Fatalf("progress after the rebuild: %+v", current.Progress)
	}
	top(knowledgeSearch(t, node, protocol.KnowledgeModeSemantic, "watering vegetables in the summer"), "garden.md")

	// Update: a renamed collection limited to Markdown drops the scans.
	definition.Name, definition.Globs = "Markdown notes", []string{"*.md"}
	cursor = knowledgeCursor(t, node)
	updated := acceptanceDo[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "update_knowledge_collection", Data: &protocol.CommandUpdateKnowledgeCollectionPayload{ID: created.ID, ExpectedRevision: current.Revision, Definition: definition}})
	events.published(t, "update", cursor)
	if updated.Definition.Name != "Markdown notes" || !slices.Equal(updated.Definition.Globs, []string{"*.md"}) || updated.Revision != current.Revision+1 {
		t.Fatalf("updated %+v", updated)
	}
	current = knowledgeReady(t, node, created.ID, func(collection protocol.KnowledgeCollection) bool {
		return collection.Progress.Generation == updated.Progress.Generation
	})
	if current.Progress.Sources != 2 || current.Progress.Completed != 2 {
		t.Fatalf("progress after the update: %+v", current.Progress)
	}
	if hidden := knowledgeSearch(t, node, protocol.KnowledgeModeKeyword, "Invoice"); len(hidden.Hits) != 0 {
		t.Fatalf("an excluded scan is still indexed: %+v", hidden)
	}
	garden := top(knowledgeSearch(t, node, protocol.KnowledgeModeKeyword, "Tomatoes"), "garden.md")

	// Restart: the collection and its index survive.
	if err := node.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	stopped = true
	node = start()
	stopped = false
	events = knowledgeFollow(t, node)
	restarted := knowledgeReady(t, node, created.ID, func(protocol.KnowledgeCollection) bool { return true })
	if restarted.Definition.Name != "Markdown notes" || restarted.Revision != current.Revision || restarted.Rebuild != current.Rebuild || restarted.Progress.Sources != 2 || restarted.Progress.Chunks != current.Progress.Chunks {
		t.Fatalf("after the restart %+v, before it %+v", restarted, current)
	}
	again := top(knowledgeSearch(t, node, protocol.KnowledgeModeSemantic, "watering vegetables in the summer"), "garden.md")
	if again.Citation.Link != garden.Citation.Link {
		t.Fatalf("the restart changed a citation: %s, was %s", again.Citation.Link, garden.Citation.Link)
	}
	if kept, failure := knowledgeRead(t, node, garden.Citation, true); failure != nil || kept.Text != texts["garden.md"] {
		t.Fatalf("citation after the restart %+v, fault %+v", kept, failure)
	}

	// A rebuild requested while paused leaves no index for the collection's
	// current state, so a citation reports the changed collection.
	held := acceptanceDo[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "pause_knowledge_collection", Data: &protocol.CommandPauseKnowledgeCollectionPayload{ID: created.ID, ExpectedRevision: restarted.Revision, Paused: true}})
	held = acceptanceDo[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "rebuild_knowledge_collection", Data: &protocol.CommandRebuildKnowledgeCollectionPayload{ID: created.ID, ExpectedRevision: held.Revision}})
	if !held.Paused {
		t.Fatalf("a rebuild resumed the paused collection: %+v", held)
	}
	_, failure = knowledgeRead(t, node, garden.Citation, false)
	acceptanceFault(t, "citation of a collection awaiting its rebuild", failure, protocol.ErrorCodeRevisionConflict, "knowledge collection changed; refresh its results")

	// Delete.
	cursor = knowledgeCursor(t, node)
	acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "delete_knowledge_collection", Data: &protocol.CommandDeleteKnowledgeCollectionPayload{ID: created.ID, ExpectedRevision: held.Revision}})
	events.published(t, "delete", cursor)
	if listed := knowledgeList(t, node); len(listed) != 0 {
		t.Fatalf("the deleted collection is listed: %+v", listed)
	}
	if gone := knowledgeSearch(t, node, protocol.KnowledgeModeKeyword, "Tomatoes"); len(gone.Hits) != 0 {
		t.Fatalf("the deleted collection answers: %+v", gone)
	}
	_, failure = knowledgeRead(t, node, garden.Citation, false)
	acceptanceFault(t, "citation of a deleted collection", failure, protocol.ErrorCodePermissionDenied, "knowledge collection is not accessible")
	if _, err := os.Stat(path("garden.md")); err != nil {
		t.Fatalf("deleting the collection touched its source: %v", err)
	}
}

// knowledgeFixture installs a package that declares the six knowledge tools.
func knowledgeFixture(t *testing.T, node *Node) protocol.PluginInfo {
	t.Helper()
	source := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"knowledge-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","scope":"host","actions":["files.read","knowledge.read","knowledge.write"],"tools":[` +
		`{"name":"knowledge_query","operation":"knowledge.query"},` +
		`{"name":"knowledge_read","operation":"knowledge.read"},` +
		`{"name":"knowledge_collections","operation":"knowledge.collections"},` +
		`{"name":"knowledge_create","operation":"knowledge.create"},` +
		`{"name":"knowledge_rebuild","operation":"knowledge.rebuild"},` +
		`{"name":"knowledge_remove","operation":"knowledge.remove"}]}}}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := node.plugins.manager.Install(t.Context(), source, "", "knowledge-fixture")
	if err != nil || len(info.Issues) != 0 || info.Extension == nil {
		t.Fatalf("knowledge fixture is invalid: %+v, %v", info.Issues, err)
	}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return info
}

// TestKnowledgeAgentTools runs one agent turn with a package that declares
// the knowledge operations. Reads run unattended; create, rebuild and remove
// wait for approval. No embedding model is installed, so retrieval is by
// keyword.
func TestKnowledgeAgentTools(t *testing.T) {
	alias := func(tool string) string { return operations.Alias("knowledge-fixture", tool) }
	query, read, collections := alias("knowledge_query"), alias("knowledge_read"), alias("knowledge_collections")
	create, rebuild, remove := alias("knowledge_create"), alias("knowledge_rebuild"), alias("knowledge_remove")
	tools := []string{query, read, collections, create, rebuild, remove}
	script := &acceptanceScript{}
	var node *Node
	// field reads a value out of a tool result the model was given.
	field := func(call string, keys ...any) any {
		var value any
		if err := json.Unmarshal([]byte(script.answered(call)), &value); err != nil {
			t.Errorf("%s answered %q: %v", call, script.answered(call), err)
			return nil
		}
		for _, key := range keys {
			switch key := key.(type) {
			case string:
				object, _ := value.(map[string]any)
				value = object[key]
			case int:
				list, _ := value.([]any)
				if key >= len(list) {
					t.Errorf("%s answered %s without %v", call, script.answered(call), keys)
					return nil
				}
				value = list[key]
			}
		}
		return value
	}
	// ready waits for the indexing pass the model would wait for.
	ready := func() {
		deadline := time.Now().Add(acceptanceWait)
		for time.Now().Before(deadline) {
			accepted, err := node.Dispatch(context.Background(), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "list_knowledge_collections"}))
			if err != nil {
				t.Errorf("list_knowledge_collections: %v", err)
				return
			}
			response := <-accepted.Completion
			listed, _ := response.Output.Data.([]protocol.KnowledgeCollection)
			if len(listed) == 1 && listed[0].Progress.Status == protocol.KnowledgeStatusReady {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Errorf("the collection was not indexed within %s", acceptanceWait)
	}
	script.steps = []acceptanceStep{
		// Discovery loads five tools unless it is asked for more.
		{"discover", "tool_search", acceptanceArguments(map[string]any{"query": "select:" + strings.Join(tools, ","), "limit": len(tools)})},
		{"create", create, acceptanceArguments(map[string]any{"name": "Docs", "source": map[string]any{"kind": "worktree", "path": "docs"}})},
		{"collections", collections, func() string { ready(); return "{}" }},
		{"query", query, acceptanceArguments(map[string]any{"query": "descaling", "mode": "keyword"})},
		{"read", read, func() string {
			return acceptanceArguments(map[string]any{
				"collection": field("query", "data", "hits", 0, "citation", "collection"),
				"chunk":      field("query", "data", "hits", 0, "citation", "chunk"),
				"revision":   field("query", "data", "hits", 0, "citation", "revision"),
			})()
		}},
		{"rebuild", rebuild, func() string {
			return acceptanceArguments(map[string]any{"id": field("collections", "data", 0, "id"), "expected_revision": field("collections", "data", 0, "revision")})()
		}},
		{"remove", remove, func() string {
			return acceptanceArguments(map[string]any{"id": field("rebuild", "data", "id"), "expected_revision": field("rebuild", "data", "revision")})()
		}},
		{id: "answer"},
	}
	node = acceptanceNode(t, filepath.Join(t.TempDir(), "profile"), script, Options{})
	defer node.Stop(context.Background())
	root, _ := codeProject(t, map[string]string{
		"docs/coffee.md":  "# Espresso machine\n\nRun a descaling cycle every three months.\n",
		"docs/bicycle.md": "# Bicycle\n\nInflate the tyres to 6 bar.\n",
		"main.go":         "package main\n\nfunc main() {}\n",
	})
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Knowledge fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	knowledgeFixture(t, node)
	session := acceptanceSession(t, node, project.ID, protocol.PermissionAsk)

	// answeredBefore lists the calls that had settled when each approval
	// was requested.
	var approvals [][]string
	frozen, results := acceptanceTurn(t, node, session, "Index the documentation and look up descaling", acceptanceWait, func(approval protocol.ConversationApproval) {
		var state store.AgentState
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var err error
			state, err = store.LoadAgentState(t.Context(), db, session.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		settled := []string{}
		for _, record := range state.Records {
			if message := record.Message(); message != nil {
				for _, block := range message.Content {
					if response, ok := block.Data.(*canonical.ToolResponse); ok {
						settled = append(settled, response.ID)
					}
				}
			}
		}
		body, _ := json.Marshal(approval)
		t.Logf("approval requested: %s", body)
		approvals = append(approvals, settled)
	})
	if !slices.ContainsFunc(frozen, func(reference protocol.PluginReference) bool { return reference.Name == "knowledge-fixture" }) {
		t.Fatalf("the turn did not freeze the fixture package: %+v", frozen)
	}
	requests := script.requests()
	if len(requests) != len(script.steps) {
		t.Fatalf("the turn made %d model requests, want %d", len(requests), len(script.steps))
	}
	for _, tool := range tools {
		// The model reads a package tool under its declared name.
		if !slices.Contains(requests[1], declaredToolName(tool)) {
			t.Errorf("%s was not offered after discovery; offered %v", tool, requests[1])
		}
	}
	for _, call := range []string{"discover", "create", "collections", "query", "read", "rebuild", "remove"} {
		if response := results[call]; response != nil {
			t.Logf("%s: status %v: %s", call, response.Status, response.Result.Value)
		}
	}
	// Exactly the three mutations asked for approval: each approval was
	// requested when only the calls before its mutation had settled, and the
	// reads between them settled without one.
	want := [][]string{
		{"discover"},
		{"discover", "create", "collections", "query", "read"},
		{"discover", "create", "collections", "query", "read", "rebuild"},
	}
	if len(approvals) != len(want) {
		t.Fatalf("the turn asked for %d approvals, want %d: %v", len(approvals), len(want), approvals)
	}
	for index := range want {
		if !slices.Equal(approvals[index], want[index]) {
			t.Fatalf("approval %d was requested after %v, want after %v", index+1, approvals[index], want[index])
		}
	}

	made := acceptanceResult(t, results, "create", create)
	collection, _ := made["data"].(map[string]any)
	owner, _ := collection["owner"].(map[string]any)
	if made["kind"] != "knowledge_collection" || collection["revision"] != float64(1) || owner["kind"] != "plugin" || owner["name"] != "knowledge-fixture" {
		t.Fatalf("knowledge_create returned %v", made)
	}
	listed := acceptanceResult(t, results, "collections", collections)
	entries, _ := listed["data"].([]any)
	if listed["kind"] != "knowledge_collections" || len(entries) != 1 {
		t.Fatalf("knowledge_collections returned %v", listed)
	}
	if entry := entries[0].(map[string]any); entry["id"] != collection["id"] || entry["progress"].(map[string]any)["status"] != "ready" || entry["progress"].(map[string]any)["sources"] != float64(2) {
		t.Fatalf("knowledge_collections listed %v", entry)
	}
	found := acceptanceResult(t, results, "query", query)
	page, _ := found["data"].(map[string]any)
	hits, _ := page["hits"].([]any)
	if found["kind"] != "knowledge_search" || len(hits) != 1 || page["mode"] != "keyword" || page["untrusted"] != true {
		t.Fatalf("knowledge_query returned %v", found)
	}
	hit := hits[0].(map[string]any)
	citation := hit["citation"].(map[string]any)
	if citation["path"] != "docs/coffee.md" || citation["collection"] != collection["id"] || !strings.HasPrefix(citation["link"].(string), "sailry-knowledge://") || !strings.Contains(hit["text"].(string), "descaling") {
		t.Fatalf("knowledge_query hit %v", hit)
	}
	cited := acceptanceResult(t, results, "read", read)
	text, _ := cited["data"].(map[string]any)
	// A model reads the complete source unless it asks for the excerpt.
	if cited["kind"] != "knowledge_citation" || text["text"] != "# Espresso machine\n\nRun a descaling cycle every three months.\n" || text["context"] != true || text["untrusted"] != true {
		t.Fatalf("knowledge_read returned %v", cited)
	}
	again := acceptanceResult(t, results, "rebuild", rebuild)
	if rebuilt, _ := again["data"].(map[string]any); again["kind"] != "knowledge_collection" || rebuilt["rebuild"] != float64(1) || rebuilt["revision"] != float64(2) {
		t.Fatalf("knowledge_rebuild returned %v", again)
	}
	removed := acceptanceResult(t, results, "remove", remove)
	if deleted, _ := removed["data"].(map[string]any); removed["kind"] != "knowledge_collection_deleted" || deleted["id"] != collection["id"] {
		t.Fatalf("knowledge_remove returned %v", removed)
	}
	if left := knowledgeList(t, node); len(left) != 0 {
		t.Fatalf("the removed collection is listed: %+v", left)
	}
	if _, err := os.Stat(filepath.Join(root, "docs", "coffee.md")); err != nil {
		t.Fatalf("removing the collection touched its source: %v", err)
	}
}
