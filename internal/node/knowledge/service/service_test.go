package service

import (
	"context"
	"database/sql"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// harness is a service over a real profile store with a fake embedding
// model at the capability seam and an injected clock.
type harness struct {
	t        *testing.T
	store    *store.Store
	service  *Service
	clock    *clockwork.FakeClock
	progress chan protocol.KnowledgeProgress

	mu        sync.Mutex
	installed bool
	encoded   int
}

// install makes the selected embedding model runnable.
func (fixture *harness) install() {
	fixture.mu.Lock()
	fixture.installed = true
	fixture.mu.Unlock()
}

// embedded counts the document texts the model encoded.
func (fixture *harness) embedded() int {
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	return fixture.encoded
}

// fakeVector places texts about vectors on one axis and all others on the
// other. A text of only spaces has no direction.
func fakeVector(text string) []float32 {
	switch {
	case strings.TrimSpace(text) == "":
		return []float32{0, 0}
	case strings.Contains(text, "向量") || strings.Contains(text, "vector"):
		return []float32{1, 0}
	}
	return []float32{0, 1}
}

func newHarness(t *testing.T) *harness {
	t.Helper()
	opened, err := store.Open(t.Context(), filepath.Join(t.TempDir(), "profile"), protocol.NodeID{1})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = opened.Close() })
	fixture := &harness{t: t, store: opened, clock: clockwork.NewFakeClock(), progress: make(chan protocol.KnowledgeProgress, 64)}
	fixture.service = New(Options{
		Index: opened.Knowledge,
		Catalog: func(ctx context.Context) (catalog []store.KnowledgeStored, err error) {
			err = opened.Node.Read(ctx, func(db *sql.Conn) error {
				catalog, err = store.ReadKnowledgeCatalog(ctx, db)
				return err
			})
			return catalog, err
		},
		Allowed: func(ctx context.Context, request protocol.Request) (allowed []store.KnowledgeStored, err error) {
			err = opened.Node.Read(ctx, func(db *sql.Conn) error {
				allowed, err = store.AllowedKnowledge(ctx, db, request)
				return err
			})
			return allowed, err
		},
		Progress: func(ctx context.Context, id protocol.KnowledgeID, generation uint64, progress protocol.KnowledgeProgress) error {
			changed := false
			err := opened.Node.Write(ctx, func(db *sql.Conn) (err error) {
				changed, err = store.RecordKnowledgeProgress(ctx, db, id, generation, progress)
				return err
			})
			if err == nil && changed {
				fixture.progress <- progress
			}
			return err
		},
		Root: func(context.Context, protocol.WorktreeID) (string, error) {
			return "", unavailable()
		},
		Choice: func() protocol.ModelsLocalChoice {
			settings := fixture.settings()
			return localmodels.Choice(&settings, protocol.ModelsKindEmbedding)
		},
		Dimension: func(model string) (int, bool) { return 2, model == localmodels.DefaultEmbedding },
		Installed: func() (map[string]bool, error) {
			fixture.mu.Lock()
			defer fixture.mu.Unlock()
			return map[string]bool{localmodels.DefaultEmbedding: fixture.installed}, nil
		},
		Encode: func(_ context.Context, _ string, _ protocol.ModelsLocalChoice, texts []string, query bool) ([][]float32, error) {
			if len(texts) > batch {
				t.Errorf("embedding batch of %d exceeds four inputs", len(texts))
			}
			if !query {
				fixture.mu.Lock()
				fixture.encoded += len(texts)
				fixture.mu.Unlock()
			}
			vectors := make([][]float32, len(texts))
			for position, text := range texts {
				vectors[position] = fakeVector(text)
			}
			return vectors, nil
		},
		Offsets: func(context.Context, string, protocol.ModelsLocalChoice, string) ([][2]int, error) { return nil, nil },
		Clock:   fixture.clock,
	})
	return fixture
}

func (fixture *harness) settings() (settings protocol.ModelsLocalSettings) {
	fixture.t.Helper()
	if err := fixture.store.Node.Read(fixture.t.Context(), func(db *sql.Conn) (err error) {
		settings, err = store.ReadLocalModelSettings(fixture.t.Context(), db)
		return err
	}); err != nil {
		fixture.t.Fatal(err)
	}
	return settings
}

// selectEmbedding saves whether the default embedding model is selected.
func (fixture *harness) selectEmbedding(enabled bool) {
	fixture.t.Helper()
	settings := fixture.settings()
	model := localmodels.DefaultEmbedding
	settings.Choices = map[protocol.ModelsKind]protocol.ModelsLocalChoice{protocol.ModelsKindEmbedding: {Enabled: enabled, Model: &model}}
	if err := fixture.store.Node.Write(fixture.t.Context(), func(db *sql.Conn) error {
		_, _, err := store.SaveLocalModelSettings(fixture.t.Context(), db, settings)
		return err
	}); err != nil {
		fixture.t.Fatal(err)
	}
}

func (fixture *harness) request(kind string, data any) protocol.Request {
	return protocol.Request{ID: protocol.NewRequestID(), Target: fixture.store.NodeID, Command: protocol.Command{Kind: kind, Data: data}}
}

// mutate applies one collection command as the Node does and returns the
// output with the kinds of the recorded events.
func (fixture *harness) mutate(kind string, data any) (output protocol.Output, kinds []string, err error) {
	fixture.t.Helper()
	err = fixture.store.Node.Write(fixture.t.Context(), func(db *sql.Conn) error {
		var events []protocol.Event
		output, events, err = store.ExecuteKnowledge(fixture.t.Context(), db, fixture.request(kind, data))
		for _, event := range events {
			kinds = append(kinds, event.Kind)
		}
		return err
	})
	return output, kinds, err
}

func (fixture *harness) create(name string, source protocol.KnowledgeSource, globs ...string) protocol.KnowledgeCollection {
	fixture.t.Helper()
	definition := protocol.DefaultKnowledgeDefinition()
	definition.Name, definition.Source, definition.Tags = name, source, []string{"selected"}
	if globs != nil {
		definition.Globs = globs
	}
	output, _, err := fixture.mutate("create_knowledge_collection", &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition})
	if err != nil {
		fixture.t.Fatal(err)
	}
	return collectionOf(fixture.t, output)
}

func collectionOf(t *testing.T, output protocol.Output) protocol.KnowledgeCollection {
	t.Helper()
	switch collection := output.Data.(type) {
	case protocol.KnowledgeCollection:
		return collection
	case *protocol.KnowledgeCollection:
		return *collection
	}
	t.Fatalf("output %q carries %T", output.Kind, output.Data)
	return protocol.KnowledgeCollection{}
}

func (fixture *harness) collections() []protocol.KnowledgeCollection {
	fixture.t.Helper()
	var collections []protocol.KnowledgeCollection
	if err := fixture.store.Node.Read(fixture.t.Context(), func(db *sql.Conn) (err error) {
		collections, err = store.ListKnowledgeCollections(fixture.t.Context(), db, fixture.request("list_knowledge_collections", nil))
		return err
	}); err != nil {
		fixture.t.Fatal(err)
	}
	return collections
}

// sweep runs one indexing pass on the test goroutine.
func (fixture *harness) sweep() {
	fixture.t.Helper()
	watches := &watches{refresh: func() {}, registered: map[string]bool{}, desired: rules{}, rules: rules{}}
	if err := fixture.service.sweep(fixture.t.Context(), watches); err != nil {
		fixture.t.Fatal(err)
	}
}

func (fixture *harness) query(text string, change func(*protocol.KnowledgeQuery)) protocol.KnowledgePage {
	fixture.t.Helper()
	query := protocol.DefaultKnowledgeQuery()
	query.Query, query.Limit = text, knowledge.DefaultLimit
	if change != nil {
		change(&query)
	}
	page, err := fixture.service.Query(fixture.t.Context(), fixture.request("query_knowledge", &query), query)
	if err != nil {
		fixture.t.Fatal(err)
	}
	return page
}

func write(t *testing.T, path, text string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(text), 0o644); err != nil {
		t.Fatal(err)
	}
}

func directory(path string) protocol.KnowledgeSource {
	return protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: path}}
}

func names(hits []protocol.KnowledgeHit) []string {
	found := []string{}
	for _, hit := range hits {
		found = append(found, filepath.Base(hit.Citation.Path))
	}
	return found
}

func wantCode(t *testing.T, err error, want protocol.ErrorCode) {
	t.Helper()
	if err == nil || code(err) != want {
		t.Fatalf("error = %v, want code %s", err, want)
	}
}

// TestKnowledgeQuery ports the keyword, binding and ranking cases of the
// Rust index tests and adds the service bounds.
func TestKnowledgeQuery(t *testing.T) {
	fixture := newHarness(t)
	ctx := t.Context()
	index := fixture.service.index
	bound := func() Binding {
		binding := Binding{ID: protocol.NewKnowledgeID(), Generation: 1, Model: "bge-small-zh", Dimension: 2}
		if err := index.Bind(ctx, binding); err != nil {
			t.Fatal(err)
		}
		return binding
	}
	put := func(binding Binding, locator, text string, vector []float32) error {
		part := Part{Title: "Source heading", Text: text, Position: protocol.KnowledgePosition{Kind: "lines", Data: protocol.KnowledgePositionLinesPayload{Start: 4, End: 8}}}
		if vector != nil {
			quantized, err := knowledge.Quantize(vector)
			if err != nil {
				t.Fatal(err)
			}
			part.Vector = &quantized
		}
		revision := strings.Repeat("a", 64)
		return index.Replace(ctx, binding, Document{Locator: locator, Revision: revision, ModifiedMs: 20, Tags: []string{"selected"}, Embedded: vector != nil}, []Part{part})
	}
	search := func(binding Binding, text string, change func(*protocol.KnowledgeQuery), embeddings ...Embedding) protocol.KnowledgePage {
		t.Helper()
		query := protocol.DefaultKnowledgeQuery()
		query.Query, query.Limit = text, knowledge.DefaultLimit
		if change != nil {
			change(&query)
		}
		page, err := index.Search(ctx, query, []Binding{binding}, embeddings, false)
		if err != nil {
			t.Fatal(err)
		}
		return page
	}

	keywords := []struct {
		name      string
		documents map[string]string
		queries   map[string][]string
	}{
		{
			name: "latin terms rank complete matches first",
			documents: map[string]string{
				"complete.md":  "Rust handles cancellation of running async operations",
				"partial.md":   "Rust language syntax",
				"unrelated.md": "Database pool limits",
			},
			queries: map[string][]string{
				"rust async cancellation": {"complete.md", "partial.md"},
				"cancellation rust async": {"complete.md", "partial.md"},
				"\" * : ()":               {},
			},
		},
		{
			name: "CJK text matches by character pairs",
			documents: map[string]string{
				"complete.md":  "管理资料，使用关键词完成检索，也提供有效方法",
				"partial.md":   "资料归档",
				"unrelated.md": "資方交付料品，检查锁方与索引方位",
			},
			queries: map[string][]string{
				"资料检索方法": {"complete.md", "partial.md"},
				"方法检索资料": {"complete.md", "partial.md"},
				"档":      {"partial.md"},
			},
		},
		{
			name: "query syntax is literal text",
			documents: map[string]string{
				"complete.md":  "Rust C++ café language",
				"unrelated.md": "secretive",
			},
			queries: map[string][]string{
				"RUST":                  {"complete.md"},
				"CAFE":                  {"complete.md"},
				"c++":                   {"complete.md"},
				"text:rust":             {"complete.md"},
				"\"rust\" OR missing*":  {"complete.md"},
				"secret*":               {},
				"rust language missing": {"complete.md"},
			},
		},
	}
	for _, group := range keywords {
		t.Run(group.name, func(t *testing.T) {
			binding := bound()
			for locator, text := range group.documents {
				if err := put(binding, locator, text, nil); err != nil {
					t.Fatal(err)
				}
			}
			for text, want := range group.queries {
				page := search(binding, text, nil)
				if found := names(page.Hits); !slices.Equal(found, want) {
					t.Errorf("query %q = %v, want %v", text, found, want)
				}
				if page.Mode != protocol.KnowledgeModeKeyword || page.Model != nil || !page.Untrusted {
					t.Errorf("query %q page = mode %s model %v untrusted %v", text, page.Mode, page.Model, page.Untrusted)
				}
			}
		})
	}

	t.Run("fusion and modes", func(t *testing.T) {
		binding := bound()
		for locator, document := range map[string]struct {
			text   string
			vector []float32
		}{
			"both.md":     {"向量检索", []float32{1, 0}},
			"semantic.md": {"embedding recall", []float32{0.8, 0.6}},
			"keyword.md":  {"向量", nil},
			"far.md":      {"unrelated", []float32{0, 1}},
		} {
			if err := put(binding, locator, document.text, document.vector); err != nil {
				t.Fatal(err)
			}
		}
		embedding := Embedding{Model: "bge-small-zh", Values: []float32{1, 0}}
		hybrid := search(binding, "向量", nil, embedding)
		// both.md leads both lists; far.md is last in the only list it is in.
		if found := names(hybrid.Hits); hybrid.Mode != protocol.KnowledgeModeHybrid || len(found) != 4 || found[0] != "both.md" || found[3] != "far.md" {
			t.Fatalf("hybrid = %s %v", hybrid.Mode, found)
		}
		if hybrid.Model == nil || *hybrid.Model != "bge-small-zh" {
			t.Fatalf("hybrid model = %v", hybrid.Model)
		}
		if hit := hybrid.Hits[0]; hit.Title != "Source heading" || hit.Citation.Link != knowledge.CitationURL(hit.Citation) || hit.Stale ||
			!strings.HasPrefix(hit.Citation.Link, "sailry-knowledge://"+binding.ID.String()+"/") {
			t.Fatalf("hit = %+v", hit)
		}
		semantic := search(binding, "向量", func(query *protocol.KnowledgeQuery) { query.Mode = protocol.KnowledgeModeSemantic }, embedding)
		if found := names(semantic.Hits); !slices.Equal(found, []string{"both.md", "semantic.md", "far.md"}) || semantic.Hits[0].Score != 1_000_000 || semantic.Hits[2].Score != 0 {
			t.Fatalf("semantic = %v %+v", found, semantic.Hits)
		}
		keyword := search(binding, "向量", func(query *protocol.KnowledgeQuery) { query.Mode = protocol.KnowledgeModeKeyword }, embedding)
		if found := names(keyword.Hits); keyword.Mode != protocol.KnowledgeModeKeyword || keyword.Model != nil || len(found) != 2 || slices.Contains(found, "far.md") {
			t.Fatalf("keyword = %s %v", keyword.Mode, found)
		}
		// A vector of another model or dimension is never compared.
		other := search(binding, "向量", nil, Embedding{Model: "other", Values: []float32{1, 0}})
		if other.Mode != protocol.KnowledgeModeKeyword || len(other.Hits) != 2 {
			t.Fatalf("other model = %s %v", other.Mode, names(other.Hits))
		}

		paged := search(binding, "向量", func(query *protocol.KnowledgeQuery) { query.Limit, query.Offset = 3, 0 }, embedding)
		if len(paged.Hits) != 3 || paged.NextOffset == nil || *paged.NextOffset != 3 {
			t.Fatalf("first page = %v next %v", names(paged.Hits), paged.NextOffset)
		}
		last := search(binding, "向量", func(query *protocol.KnowledgeQuery) { query.Limit, query.Offset = 3, 3 }, embedding)
		if found := names(last.Hits); !slices.Equal(found, []string{"far.md"}) || last.NextOffset != nil {
			t.Fatalf("last page = %v next %v", found, last.NextOffset)
		}

		filtered := []struct {
			name   string
			change func(*protocol.KnowledgeQuery)
			want   int
		}{
			{"matching tag", func(query *protocol.KnowledgeQuery) { query.Tags = []string{"selected"} }, 4},
			{"missing tag", func(query *protocol.KnowledgeQuery) { query.Tags = []string{"selected", "absent"} }, 0},
			{"path prefix", func(query *protocol.KnowledgeQuery) { prefix := "both"; query.Path = &prefix }, 1},
			{"modified after", func(query *protocol.KnowledgeQuery) { after := uint64(21); query.ModifiedAfterMs = &after }, 0},
			{"modified before", func(query *protocol.KnowledgeQuery) { before := uint64(20); query.ModifiedBeforeMs = &before }, 4},
			{"other collection", func(query *protocol.KnowledgeQuery) {
				query.Collections = []protocol.KnowledgeID{protocol.NewKnowledgeID()}
			}, 0},
		}
		for _, filter := range filtered {
			if page := search(binding, "向量", filter.change, embedding); len(page.Hits) != filter.want {
				t.Errorf("%s = %v, want %d hits", filter.name, names(page.Hits), filter.want)
			}
		}
	})

	t.Run("bindings admit only what they indexed", func(t *testing.T) {
		binding := bound()
		if err := put(binding, "source.md", "向量", []float32{1, 0}); err != nil {
			t.Fatal(err)
		}
		embedding := Embedding{Model: "bge-small-zh", Values: []float32{1, 0}}
		chunk := search(binding, "向量", nil, embedding).Hits[0].Citation
		read := func(binding Binding) error {
			_, err := index.citation(ctx, binding, protocol.KnowledgeCitationRequest{Collection: chunk.Collection, Chunk: chunk.Chunk, Revision: chunk.Revision, Limit: 1})
			return err
		}

		// A paused collection keeps its cached text as keyword results.
		paused := binding
		paused.Generation, paused.Paused = 2, true
		if page := search(paused, "向量", nil, embedding); page.Mode != protocol.KnowledgeModeKeyword || len(page.Hits) != 1 {
			t.Fatalf("paused = %s %v", page.Mode, names(page.Hits))
		}
		if err := read(paused); err != nil {
			t.Fatal(err)
		}

		// A resumed generation has nothing until the worker binds it.
		resumed := binding
		resumed.Generation = 3
		if page := search(resumed, "向量", nil, embedding); len(page.Hits) != 0 {
			t.Fatalf("unbound generation = %v", names(page.Hits))
		}
		if err := index.Bind(ctx, resumed); err != nil {
			t.Fatal(err)
		}
		wantCode(t, put(binding, "late.md", "向量", nil), protocol.ErrorCodeCancelled)
		if page := search(resumed, "向量", nil, embedding); page.Mode != protocol.KnowledgeModeHybrid || len(page.Hits) != 1 {
			t.Fatalf("resumed = %s %v", page.Mode, names(page.Hits))
		}

		// Another model keeps the text and drops the vectors.
		keyword := resumed
		keyword.Model, keyword.Dimension = "", 0
		if err := index.Bind(ctx, keyword); err != nil {
			t.Fatal(err)
		}
		if sources, chunks, err := index.Counts(ctx, keyword); err != nil || sources != 1 || chunks != 1 {
			t.Fatalf("counts = %d %d %v", sources, chunks, err)
		}
		document := Document{Locator: "source.md", Revision: strings.Repeat("a", 64), ModifiedMs: 20, Tags: []string{"selected"}}
		if unchanged, err := index.Unchanged(ctx, keyword, document); err != nil || !unchanged {
			t.Fatalf("keyword document unchanged = %v %v", unchanged, err)
		}
		document.Embedded = true
		if unchanged, err := index.Unchanged(ctx, keyword, document); err != nil || unchanged {
			t.Fatalf("embedded document unchanged = %v %v", unchanged, err)
		}
		if page := search(keyword, "向量", nil, embedding); page.Mode != protocol.KnowledgeModeKeyword || len(page.Hits) != 1 {
			t.Fatalf("keyword only = %s %v", page.Mode, names(page.Hits))
		}

		// A changed source capture or a rebuild invalidates everything.
		for name, change := range map[string]func(*Binding){
			"scope":   func(binding *Binding) { binding.Scope[0] = 1 },
			"rebuild": func(binding *Binding) { binding.Rebuild++ },
		} {
			changed := keyword
			change(&changed)
			if page := search(changed, "向量", nil); len(page.Hits) != 0 {
				t.Fatalf("changed %s = %v", name, names(page.Hits))
			}
			wantCode(t, read(changed), protocol.ErrorCodeRevisionConflict)
		}
		rebuilt := keyword
		rebuilt.Rebuild++
		if err := index.Bind(ctx, rebuilt); err != nil {
			t.Fatal(err)
		}
		if sources, chunks, err := index.Counts(ctx, rebuilt); err != nil || sources != 0 || chunks != 0 {
			t.Fatalf("rebuilt counts = %d %d %v", sources, chunks, err)
		}
		if page := search(rebuilt, "向量", nil); len(page.Hits) != 0 {
			t.Fatalf("rebuilt keyword index = %v", names(page.Hits))
		}
		wantCode(t, read(rebuilt), protocol.ErrorCodeNotFound)
	})

	t.Run("service bounds", func(t *testing.T) {
		root := t.TempDir()
		write(t, filepath.Join(root, "vectors.md"), "# Vectors\n\n向量 search finds related text\n")
		write(t, filepath.Join(root, "notes.md"), "plain notes about 向量\n")
		fixture.install()
		collection := fixture.create("Docs", directory(root))
		if page := fixture.query("向量", nil); !page.Indexing || len(page.Hits) != 0 {
			t.Fatalf("pending = %+v", page)
		}
		fixture.sweep()
		page := fixture.query("向量", nil)
		if page.Mode != protocol.KnowledgeModeHybrid || page.Indexing || len(page.Hits) != 2 || page.Model == nil || *page.Model != localmodels.DefaultEmbedding {
			t.Fatalf("hybrid = %+v", page)
		}
		if hit := page.Hits[0]; hit.Citation.Collection != collection.ID || hit.Stale {
			t.Fatalf("hit = %+v", hit)
		}
		if page := fixture.query("向量", func(query *protocol.KnowledgeQuery) { query.Limit = 1000 }); len(page.Hits) != 2 || page.NextOffset != nil {
			t.Fatalf("clamped limit = %+v", page)
		}
		if page := fixture.query("向量", func(query *protocol.KnowledgeQuery) { query.Limit = 1 }); len(page.Hits) != 1 || page.NextOffset == nil || *page.NextOffset != 1 {
			t.Fatalf("one hit = %+v", page)
		}
		if page := fixture.query("向量", func(query *protocol.KnowledgeQuery) {
			query.Collections = []protocol.KnowledgeID{protocol.NewKnowledgeID()}
		}); len(page.Hits) != 0 {
			t.Fatalf("unknown collection = %+v", page)
		}
		for name, change := range map[string]func(*protocol.KnowledgeQuery){
			"blank query": func(query *protocol.KnowledgeQuery) { query.Query = " " },
			"zero limit":  func(query *protocol.KnowledgeQuery) { query.Limit = 0 },
			"inverted range": func(query *protocol.KnowledgeQuery) {
				after, before := uint64(2), uint64(1)
				query.ModifiedAfterMs, query.ModifiedBeforeMs = &after, &before
			},
		} {
			query := protocol.DefaultKnowledgeQuery()
			query.Query, query.Limit = "向量", knowledge.DefaultLimit
			change(&query)
			if _, err := fixture.service.Query(ctx, fixture.request("query_knowledge", &query), query); err == nil || code(err) != protocol.ErrorCodeInvalidRequest {
				t.Errorf("%s error = %v", name, err)
			}
		}
	})
}

// TestCitationRecheck reads citations through the current source and
// reports stale revisions and missing chunks as the Rust service does.
func TestCitationRecheck(t *testing.T) {
	fixture := newHarness(t)
	ctx := t.Context()
	root := t.TempDir()
	path := filepath.Join(root, "guide.md")
	write(t, path, "# Guide\n\nfirst line about citations\nsecond line\n")
	write(t, filepath.Join(root, "skipped.txt"), "citations outside the glob\n")
	collection := fixture.create("Guide", directory(root), "*.md")
	fixture.sweep()

	page := fixture.query("citations", nil)
	if found := names(page.Hits); !slices.Equal(found, []string{"guide.md"}) {
		t.Fatalf("hits = %v", found)
	}
	citation := page.Hits[0].Citation
	read := func(change func(*protocol.KnowledgeCitationRequest)) (protocol.KnowledgeCitationPage, error) {
		input := protocol.KnowledgeCitationRequest{Collection: citation.Collection, Chunk: citation.Chunk, Revision: citation.Revision, Limit: knowledge.CitationLimit}
		if change != nil {
			change(&input)
		}
		return fixture.service.Citation(ctx, fixture.request("read_knowledge_citation", &input), input)
	}

	cited, err := read(nil)
	if err != nil {
		t.Fatal(err)
	}
	if cited.Title != "Guide" || cited.Text != "# Guide\n\nfirst line about citations\nsecond line" || cited.Citation.Path != path || cited.Citation.Link != citation.Link ||
		cited.NextOffset != nil || cited.Context || !cited.Untrusted || cited.Worktree != nil {
		t.Fatalf("citation = %+v", cited)
	}
	complete, err := read(func(input *protocol.KnowledgeCitationRequest) { input.Context = true })
	if err != nil || complete.Text != "# Guide\n\nfirst line about citations\nsecond line\n" || !complete.Context {
		t.Fatalf("context = %+v %v", complete, err)
	}
	first, err := read(func(input *protocol.KnowledgeCitationRequest) { input.Limit = 7 })
	if err != nil || first.Text != "# Guide" || first.NextOffset == nil || *first.NextOffset != 7 {
		t.Fatalf("first page = %+v %v", first, err)
	}
	rest, err := read(func(input *protocol.KnowledgeCitationRequest) { input.Offset = *first.NextOffset })
	if err != nil || first.Text+rest.Text != cited.Text || rest.Offset != 7 || rest.NextOffset != nil {
		t.Fatalf("second page = %+v %v", rest, err)
	}

	invalid := map[string]func(*protocol.KnowledgeCitationRequest){
		"chunk is not a hash":    func(input *protocol.KnowledgeCitationRequest) { input.Chunk = "chunk" },
		"revision is not a hash": func(input *protocol.KnowledgeCitationRequest) { input.Revision = strings.ToUpper(input.Revision) },
		"zero limit":             func(input *protocol.KnowledgeCitationRequest) { input.Limit = 0 },
		"offset past the text":   func(input *protocol.KnowledgeCitationRequest) { input.Offset = 1 << 20 },
	}
	for name, change := range invalid {
		if _, err := read(change); err == nil || code(err) != protocol.ErrorCodeInvalidRequest {
			t.Errorf("%s error = %v", name, err)
		}
	}
	_, err = read(func(input *protocol.KnowledgeCitationRequest) { input.Collection = protocol.NewKnowledgeID() })
	wantCode(t, err, protocol.ErrorCodePermissionDenied)

	drain := func() {
		select {
		case <-fixture.service.wake:
		default:
		}
	}
	refreshed := func() bool {
		select {
		case <-fixture.service.wake:
			return true
		default:
			return false
		}
	}
	drain()

	// An indexed chunk cited at another revision is stale.
	_, err = read(func(input *protocol.KnowledgeCitationRequest) { input.Revision = strings.Repeat("0", 64) })
	wantCode(t, err, protocol.ErrorCodeRevisionConflict)
	if !refreshed() {
		t.Fatal("a stale revision did not request indexing")
	}
	// A chunk that was never indexed is missing.
	_, err = read(func(input *protocol.KnowledgeCitationRequest) { input.Chunk = strings.Repeat("0", 64) })
	wantCode(t, err, protocol.ErrorCodeNotFound)
	if !refreshed() {
		t.Fatal("a missing chunk did not request indexing")
	}

	// The source changed after it was indexed: the bytes are rechecked.
	write(t, path, "# Guide\n\nrewritten line about citations\nsecond line\n")
	_, err = read(nil)
	wantCode(t, err, protocol.ErrorCodeRevisionConflict)
	if !refreshed() {
		t.Fatal("a changed source did not request indexing")
	}

	// Reindexing replaces the chunk; the old citation is no longer indexed.
	fixture.sweep()
	_, err = read(nil)
	wantCode(t, err, protocol.ErrorCodeNotFound)
	fresh := fixture.query("citations", nil).Hits[0].Citation
	if fresh.Chunk == citation.Chunk || fresh.Revision == citation.Revision || fresh.Link == citation.Link {
		t.Fatalf("reindexed citation kept its identity: %+v", fresh)
	}
	citation = fresh
	if cited, err := read(nil); err != nil || !strings.Contains(cited.Text, "rewritten line") {
		t.Fatalf("fresh citation = %+v %v", cited, err)
	}

	// A file the definition no longer selects is not readable.
	definition := collection.Definition
	definition.Globs = []string{"*.txt"}
	current := fixture.collections()[0]
	if _, _, err := fixture.mutate("update_knowledge_collection", &protocol.CommandUpdateKnowledgeCollectionPayload{ID: collection.ID, ExpectedRevision: current.Revision, Definition: definition}); err != nil {
		t.Fatal(err)
	}
	_, err = read(nil)
	wantCode(t, err, protocol.ErrorCodeRevisionConflict)

	// A removed source is reported as changed, not as a missing file.
	definition.Globs = []string{"*.md"}
	current = fixture.collections()[0]
	if _, _, err := fixture.mutate("update_knowledge_collection", &protocol.CommandUpdateKnowledgeCollectionPayload{ID: collection.ID, ExpectedRevision: current.Revision, Definition: definition}); err != nil {
		t.Fatal(err)
	}
	fixture.sweep()
	citation = fixture.query("citations", nil).Hits[0].Citation
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	_, err = read(nil)
	wantCode(t, err, protocol.ErrorCodeRevisionConflict)
}

// TestCollectionLifecycle drives a collection through the commands and the
// worker: keyword only without a model, vectors once one is selected and
// installed, pause, rebuild and delete.
func TestCollectionLifecycle(t *testing.T) {
	fixture := newHarness(t)
	ctx := t.Context()
	root := t.TempDir()
	write(t, filepath.Join(root, "vectors.md"), "# Vectors\n\n向量 search finds related text\n")
	write(t, filepath.Join(root, "notes", "plain.txt"), "plain notes\n")
	write(t, filepath.Join(root, "ignored.md"), "ignored 向量\n")
	write(t, filepath.Join(root, ".gitignore"), "ignored.md\n")
	write(t, filepath.Join(root, ".env"), "SECRET=向量\n")
	write(t, filepath.Join(root, "binary.dat"), "\x00\x01向量")
	fixture.selectEmbedding(false)

	// A collection is created pending and announced.
	definition := protocol.DefaultKnowledgeDefinition()
	definition.Name, definition.Source = "Docs", directory(root)
	output, events, err := fixture.mutate("create_knowledge_collection", &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition})
	if err != nil {
		t.Fatal(err)
	}
	created := collectionOf(t, output)
	if output.Kind != "knowledge_collection" || !slices.Equal(events, []string{"knowledge_collections_changed"}) ||
		created.Revision != 1 || created.Progress.Status != protocol.KnowledgeStatusPending || created.Paused || created.Owner.Kind != "controller" {
		t.Fatalf("created = %s %v %+v", output.Kind, events, created)
	}
	definition.Name = " "
	_, _, err = fixture.mutate("create_knowledge_collection", &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition})
	wantCode(t, err, protocol.ErrorCodeInvalidRequest)
	definition.Name, definition.Source = "Relative", directory("relative")
	_, _, err = fixture.mutate("create_knowledge_collection", &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition})
	wantCode(t, err, protocol.ErrorCodeInvalidRequest)

	// The worker debounces the request, indexes and reports progress.
	fixture.service.Start()
	t.Cleanup(fixture.service.Close)
	next := func() protocol.KnowledgeProgress {
		t.Helper()
		for {
			// The worker either reports progress or waits on the clock for
			// its quiet interval; the test advances only the latter.
			waiting, cancel := context.WithCancel(ctx)
			quiet := make(chan struct{})
			go func() {
				if fixture.clock.BlockUntilContext(waiting, 1) == nil {
					close(quiet)
				}
			}()
			select {
			case progress := <-fixture.progress:
				cancel()
				return progress
			case <-quiet:
				cancel()
				fixture.clock.Advance(250 * time.Millisecond)
			}
		}
	}
	until := func(status protocol.KnowledgeStatus) protocol.KnowledgeProgress {
		t.Helper()
		for {
			if progress := next(); progress.Status == status {
				return progress
			}
		}
	}
	if progress := next(); progress.Status != protocol.KnowledgeStatusIndexing || progress.Model != nil {
		t.Fatalf("first progress = %+v", progress)
	}
	ready := until(protocol.KnowledgeStatusReady)
	// vectors.md, notes/plain.txt and .gitignore; binary.dat is skipped.
	if ready.Sources != 3 || ready.Completed != 3 || ready.Chunks != 3 || ready.Skipped != 1 || ready.Model != nil || ready.Generation != created.Progress.Generation {
		t.Fatalf("keyword only = %+v", ready)
	}
	if count := fixture.embedded(); count != 0 {
		t.Fatalf("keyword-only indexing embedded %d texts", count)
	}
	page := fixture.query("向量", nil)
	if found := names(page.Hits); page.Mode != protocol.KnowledgeModeKeyword || page.Model != nil || !slices.Equal(found, []string{"vectors.md"}) {
		t.Fatalf("keyword only query = %s %v", page.Mode, found)
	}
	if start, end, ranged := lines(page.Hits[0].Citation.Position); page.Hits[0].Title != "Vectors" || !ranged || start != 1 || end != 3 {
		t.Fatalf("hit = %+v at lines %d to %d", page.Hits[0], start, end)
	}

	// Selecting an embedding model restarts indexing; it stays keyword
	// only until the model is installed.
	fixture.selectEmbedding(true)
	selected := fixture.collections()[0]
	if selected.Progress.Generation <= ready.Generation || selected.Progress.Status != protocol.KnowledgeStatusPending || selected.Revision != created.Revision+1 {
		t.Fatalf("selection change = %+v", selected)
	}
	fixture.service.Invalidate()
	if progress := until(protocol.KnowledgeStatusReady); progress.Model != nil || progress.Chunks != 3 || fixture.embedded() != 0 {
		t.Fatalf("selected but not installed = %+v, %d texts", progress, fixture.embedded())
	}
	fixture.install()
	fixture.service.Invalidate()
	embedded := until(protocol.KnowledgeStatusReady)
	if embedded.Model == nil || *embedded.Model != localmodels.DefaultEmbedding || embedded.Sources != 3 || embedded.Chunks != 3 || fixture.embedded() != 3 {
		t.Fatalf("embedded = %+v, %d texts", embedded, fixture.embedded())
	}
	page = fixture.query("向量", nil)
	if found := names(page.Hits); page.Mode != protocol.KnowledgeModeHybrid || page.Model == nil || len(found) != 3 || found[0] != "vectors.md" {
		t.Fatalf("hybrid query = %s %v", page.Mode, found)
	}

	// A new file is indexed; the published sources are reused.
	write(t, filepath.Join(root, "notes", "added.md"), "added 向量 text\n")
	fixture.service.Refresh()
	if progress := until(protocol.KnowledgeStatusReady); progress.Sources != 4 || progress.Chunks != 4 || fixture.embedded() != 4 {
		t.Fatalf("added file = %+v, %d texts", progress, fixture.embedded())
	}

	// A stale revision is a conflict and changes nothing.
	current := fixture.collections()[0]
	_, _, err = fixture.mutate("pause_knowledge_collection", &protocol.CommandPauseKnowledgeCollectionPayload{ID: current.ID, ExpectedRevision: current.Revision + 1, Paused: true})
	wantCode(t, err, protocol.ErrorCodeRevisionConflict)

	// A paused collection keeps its cached text as keyword results.
	output, events, err = fixture.mutate("pause_knowledge_collection", &protocol.CommandPauseKnowledgeCollectionPayload{ID: current.ID, ExpectedRevision: current.Revision, Paused: true})
	if err != nil || !slices.Equal(events, []string{"knowledge_collections_changed"}) {
		t.Fatalf("pause = %v %v", events, err)
	}
	paused := collectionOf(t, output)
	if !paused.Paused || paused.Revision != current.Revision+1 {
		t.Fatalf("paused = %+v", paused)
	}
	if _, events, err := fixture.mutate("pause_knowledge_collection", &protocol.CommandPauseKnowledgeCollectionPayload{ID: current.ID, ExpectedRevision: paused.Revision, Paused: true}); err != nil || len(events) != 0 {
		t.Fatalf("repeated pause = %v %v", events, err)
	}
	fixture.service.Invalidate()
	if progress := paused.Progress; progress.Status != protocol.KnowledgeStatusPaused || progress.Sources != 4 || progress.Completed != 4 || progress.Chunks != 4 || progress.Model != nil {
		t.Fatalf("paused progress = %+v", progress)
	}
	if page := fixture.query("向量", nil); page.Mode != protocol.KnowledgeModeKeyword || len(page.Hits) != 2 {
		t.Fatalf("paused query = %s %v", page.Mode, names(page.Hits))
	}

	// Resuming and rebuilding index the collection again from nothing.
	current = fixture.collections()[0]
	if _, _, err := fixture.mutate("pause_knowledge_collection", &protocol.CommandPauseKnowledgeCollectionPayload{ID: current.ID, ExpectedRevision: current.Revision, Paused: false}); err != nil {
		t.Fatal(err)
	}
	fixture.service.Invalidate()
	until(protocol.KnowledgeStatusReady)
	if count := fixture.embedded(); count != 4 {
		t.Fatalf("resuming embedded again: %d texts", count)
	}
	current = fixture.collections()[0]
	output, _, err = fixture.mutate("rebuild_knowledge_collection", &protocol.CommandRebuildKnowledgeCollectionPayload{ID: current.ID, ExpectedRevision: current.Revision})
	if err != nil {
		t.Fatal(err)
	}
	rebuilt := collectionOf(t, output)
	if rebuilt.Rebuild != current.Rebuild+1 || rebuilt.Progress.Status != protocol.KnowledgeStatusPending || rebuilt.Progress.Generation <= current.Progress.Generation {
		t.Fatalf("rebuilt = %+v", rebuilt)
	}
	if page := fixture.query("向量", nil); !page.Indexing || len(page.Hits) != 0 {
		t.Fatalf("query during rebuild = %+v", page)
	}
	fixture.service.Invalidate()
	if progress := until(protocol.KnowledgeStatusReady); progress.Sources != 4 || progress.Chunks != 4 || fixture.embedded() != 8 {
		t.Fatalf("rebuild = %+v, %d texts", progress, fixture.embedded())
	}

	// Deleting removes the collection and its retrieval data.
	current = fixture.collections()[0]
	output, events, err = fixture.mutate("delete_knowledge_collection", &protocol.CommandDeleteKnowledgeCollectionPayload{ID: current.ID, ExpectedRevision: current.Revision})
	if err != nil || output.Kind != "knowledge_collection_deleted" || !slices.Equal(events, []string{"knowledge_collections_changed"}) {
		t.Fatalf("delete = %s %v %v", output.Kind, events, err)
	}
	if remaining := fixture.collections(); len(remaining) != 0 {
		t.Fatalf("collections after delete = %+v", remaining)
	}
	fixture.service.Close()
	fixture.sweep()
	var sources int
	if err := fixture.store.Knowledge.Read(ctx, func(db *sql.Conn) error {
		return db.QueryRowContext(ctx, "SELECT (SELECT count(*) FROM collections)+(SELECT count(*) FROM sources)+(SELECT count(*) FROM chunks)").Scan(&sources)
	}); err != nil || sources != 0 {
		t.Fatalf("retrieval rows after delete = %d %v", sources, err)
	}
}
