package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	knowledgeservice "github.com/sailry/sailry-harness/internal/node/knowledge/service"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// knowledgeStorageScript answers each model request of a turn with the next
// planned tool calls, then with text.
type knowledgeStorageScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	steps   [][][3]string
	results map[string]string
}

func (script *knowledgeStorageScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found {
				if text, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output); found {
					script.results[result.ToolCallID] = text.Text
				}
			}
		}
	}
	var calls [][3]string
	if len(script.steps) != 0 {
		calls, script.steps = script.steps[0], script.steps[1:]
	}
	script.mu.Unlock()
	return func(yield func(fantasy.StreamPart) bool) {
		for _, call := range calls {
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: call[0], ToolCallName: call[1], ToolCallInput: call[2]}) {
				return
			}
		}
		if len(calls) == 0 && !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

// seedKnowledgeFixtureEmbedding prepares a profile whose worker uses a fake
// embedding runtime for Granite, with the same saved choice after restart.
func seedKnowledgeFixtureEmbedding(t *testing.T, path string) {
	t.Helper()
	ctx := t.Context()
	profile, err := OpenProfile(ctx, path)
	if err != nil {
		t.Fatal(err)
	}
	err = profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		settings, err := store.ReadLocalModelSettings(ctx, db)
		if err != nil {
			return err
		}
		settings.Choices[protocol.ModelsKindEmbedding] = protocol.ModelsLocalChoice{Enabled: true, Model: new(localmodels.DefaultEmbedding)}
		_, _, err = store.SaveLocalModelSettings(ctx, db, settings)
		return err
	})
	if err := errors.Join(err, profile.Close()); err != nil {
		t.Fatal(err)
	}
}

// TestKnowledgeStorage indexes plugin storage through a real Node with a fake
// embedding model: the bundled memory package's own tools feed its automatic
// semantic collection, and a fixture package owns an explicit storage
// collection.
func TestKnowledgeStorage(t *testing.T) {
	script := &knowledgeStorageScript{results: map[string]string{}}
	var encoded atomic.Int64
	type catalogBarrier struct {
		ready, waiting chan struct{}
		once           sync.Once
	}
	firstBatch := &catalogBarrier{ready: make(chan struct{}), waiting: make(chan struct{})}
	var catalogGate atomic.Pointer[catalogBarrier]
	catalogGate.Store(firstBatch)
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	profile := filepath.Join(t.TempDir(), "profile")
	seedKnowledgeFixtureEmbedding(t, profile)
	ctx := t.Context()
	var node *Node
	start := func(t *testing.T) {
		t.Helper()
		started, err := StartWithOptions(ctx, profile, Options{
			Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
			Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
				return protocol.ProcessCompletion{}, errors.New("storage knowledge must not launch a process")
			}},
			// Texts about vectors lie on one axis and all others on the other.
			knowledge: func(options *knowledgeservice.Options) {
				catalog := options.Catalog
				options.Catalog = func(ctx context.Context) ([]store.KnowledgeStored, error) {
					barrier := catalogGate.Load()
					barrier.once.Do(func() { close(barrier.waiting) })
					select {
					case <-barrier.ready:
						return catalog(ctx)
					case <-ctx.Done():
						return nil, ctx.Err()
					}
				}
				selected := localmodels.DefaultEmbedding
				options.Dimension = func(model string) (int, string, bool) { return 2, "fixture", model == selected }
				options.Installed = func() (map[string]bool, error) { return map[string]bool{selected: true}, nil }
				options.Offsets = func(context.Context, string, protocol.ModelsLocalChoice, string) ([][2]int, error) { return nil, nil }
				options.Encode = func(_ context.Context, _ string, _ protocol.ModelsLocalChoice, texts []string, query bool) ([][]float32, error) {
					if !query {
						encoded.Add(int64(len(texts)))
					}
					vectors := make([][]float32, len(texts))
					for position, text := range texts {
						vectors[position] = []float32{0, 1}
						if strings.Contains(text, "vector") || strings.Contains(text, "向量") {
							vectors[position] = []float32{1, 0}
						}
					}
					return vectors, nil
				}
			},
		})
		if err != nil {
			t.Fatal(err)
		}
		node = started
	}
	start(t)
	defer func() { _ = node.Stop(context.Background()) }()

	fixture := func(name string, actions ...string) string {
		directory := t.TempDir()
		manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["` + strings.Join(actions, `","`) + `"]}}}`
		if err := os.WriteFile(filepath.Join(directory, "plugin.json"), []byte(manifest), 0o600); err != nil {
			t.Fatal(err)
		}
		return directory
	}
	const notes, reader, blind = "knowledge-notes-fixture", "knowledge-reader-fixture", "knowledge-blind-fixture"
	references := map[string]protocol.PluginReference{}
	// Memory is the bundled package the Node installed with its own manifest.
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		info, err := store.ReadPlugin(t.Context(), db, "memory")
		if err == nil && (info == nil || !info.Summary.Enabled || info.Origin == nil || info.Origin.Kind != "bundled") {
			err = errors.New("bundled memory package is not enabled")
		}
		if err == nil {
			references["memory"] = plugins.PackageReference(*info)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	for name, source := range map[string]string{
		notes:  fixture(notes, "storage.read", "storage.write", "conversation.read", "conversation.control", "knowledge.read", "knowledge.write"),
		reader: fixture(reader, "storage.read", "knowledge.read", "knowledge.write"),
		blind:  fixture(blind, "knowledge.read", "knowledge.write"),
	} {
		info, err := node.plugins.manager.Install(t.Context(), source, "", name)
		if err != nil || len(info.Issues) != 0 || info.Extension == nil {
			t.Fatalf("%s is invalid: %+v, %v", name, info.Issues, err)
		}
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			var err error
			info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
			return err
		}); err != nil {
			t.Fatalf("%s was not installed: %v", name, err)
		}
		references[name] = plugins.PackageReference(info)
	}

	// call runs one command as the controller, or as the named package.
	// conversation, when set, is the conversation the notes package acts in.
	var conversation *protocol.Session
	call := func(t *testing.T, owner string, kind string, data any) (protocol.Output, *protocol.Fault) {
		t.Helper()
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: kind, Data: data})
		if owner != "" {
			scope := protocol.DefaultPluginContext()
			scope.Package = references[owner]
			if owner == notes && conversation != nil {
				scope.Session, scope.Worktree = &conversation.ID, &conversation.Worktree
			}
			request.Plugin = &scope
		}
		accepted, err := node.Dispatch(t.Context(), node.ID(), request)
		if err != nil {
			var fault *protocol.Fault
			if !errors.As(err, &fault) {
				t.Fatal(err)
			}
			return protocol.Output{}, fault
		}
		response := <-accepted.Completion
		return response.Output, response.Fault
	}
	must := func(t *testing.T, owner string, kind string, data any) protocol.Output {
		t.Helper()
		output, fault := call(t, owner, kind, data)
		if fault != nil {
			t.Fatalf("%s as %q failed: %v", kind, owner, fault)
		}
		return output
	}
	denied := func(t *testing.T, fault *protocol.Fault, code protocol.ErrorCode, fragment string) {
		t.Helper()
		if fault == nil || fault.Code != code || !strings.Contains(fault.Message, fragment) {
			t.Fatalf("fault = %v, want %s containing %q", fault, code, fragment)
		}
	}
	eventually := func(t *testing.T, what string, done func() bool) {
		t.Helper()
		for deadline := time.Now().Add(30 * time.Second); !done(); time.Sleep(20 * time.Millisecond) {
			if time.Now().After(deadline) {
				t.Fatalf("%s did not happen", what)
			}
		}
	}
	collection := func(t *testing.T, name string) *protocol.KnowledgeCollection {
		t.Helper()
		listed, _ := dispatchPayload[[]protocol.KnowledgeCollection](must(t, "", "list_knowledge_collections", nil).Data)
		for _, collection := range listed {
			if collection.Definition.Name == name {
				return &collection
			}
		}
		return nil
	}
	ready := func(t *testing.T, name string, sources uint64) protocol.KnowledgeCollection {
		t.Helper()
		var found protocol.KnowledgeCollection
		eventually(t, name+" indexing", func() bool {
			current := collection(t, name)
			if current == nil || current.Progress.Status != protocol.KnowledgeStatusReady || current.Progress.Sources != sources {
				return false
			}
			found = *current
			return true
		})
		return found
	}
	query := func(t *testing.T, owner, text string, mode protocol.KnowledgeMode) (protocol.KnowledgePage, *protocol.Fault) {
		t.Helper()
		output, fault := call(t, owner, "query_knowledge", &protocol.KnowledgeQuery{Query: text, Collections: []protocol.KnowledgeID{}, Mode: mode, Tags: []string{}, Limit: 5})
		page, _ := dispatchPayload[protocol.KnowledgePage](output.Data)
		return page, fault
	}
	hit := func(page protocol.KnowledgePage, path string) *protocol.KnowledgeHit {
		for _, hit := range page.Hits {
			if hit.Citation.Path == path {
				return &hit
			}
		}
		return nil
	}
	cite := func(t *testing.T, owner string, citation protocol.KnowledgeCitation) (protocol.KnowledgeCitationPage, *protocol.Fault) {
		t.Helper()
		output, fault := call(t, owner, "read_knowledge_citation", &protocol.KnowledgeCitationRequest{Collection: citation.Collection, Chunk: citation.Chunk, Revision: citation.Revision, Limit: 4096})
		page, _ := dispatchPayload[protocol.KnowledgeCitationPage](output.Data)
		return page, fault
	}
	values := func(t *testing.T, owner string, text string) protocol.PluginStorageSearchPage {
		t.Helper()
		search := protocol.DefaultPluginStorageSearch()
		search.Query, search.Weights, search.Limit = &text, [2]uint16{3, 1}, 10
		page, _ := dispatchPayload[protocol.PluginStorageSearchPage](must(t, owner, "search_plugin_values", &search).Data)
		return page
	}
	storage := func(scope protocol.PluginStorageScope, name, prefix string) *protocol.CommandCreateKnowledgeCollectionPayload {
		return &protocol.CommandCreateKnowledgeCollectionPayload{Definition: protocol.KnowledgeDefinition{
			Name: name, Globs: []string{}, Tags: []string{}, SharedWith: []string{},
			Source: protocol.KnowledgeSource{Kind: "storage", Data: &protocol.KnowledgeSourceStoragePayload{Scope: scope, Prefix: prefix}},
		}}
	}

	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project, _ := dispatchPayload[protocol.Project](must(t, "", "create_project", protocol.ProjectsDraft{
		Name: "Storage knowledge", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}).Data)
	// turn runs one conversation turn in which the model makes the planned
	// tool calls, one list per model request.
	turn := func(t *testing.T, steps ...[][3]string) {
		t.Helper()
		script.mu.Lock()
		script.steps = steps
		script.mu.Unlock()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionFull
		session, _ := dispatchPayload[protocol.Session](must(t, "", "create_session", protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}).Data)
		must(t, "", "submit_turn", protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Use memory"}})
		eventually(t, "memory turn", func() bool {
			var history protocol.ConversationHistory
			if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, run := range history.Page.Runs {
				switch run.Status {
				case protocol.ConversationStatusCompleted:
					return true
				case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
					t.Fatalf("memory turn failed: %+v", run)
				}
			}
			return false
		})
		script.mu.Lock()
		defer script.mu.Unlock()
		if len(script.steps) != 0 {
			t.Fatalf("memory turn left %d model requests unused", len(script.steps))
		}
	}
	result := func(id string) string {
		script.mu.Lock()
		defer script.mu.Unlock()
		return script.results[id]
	}
	memory := func(name string) string { return operations.Alias("memory", name) }
	// talk is the conversation whose values "Conversation notes" indexes.
	var talk protocol.Session
	const preference = "Use the vector editor theme"

	if !t.Run("memory tools feed an automatic collection", func(t *testing.T) {
		if collection(t, "memory storage") != nil {
			t.Fatal("automatic collection exists before its first indexed value")
		}
		turn(t,
			[][3]string{{"save-theme", memory("save_memory"), `{"kind":"user","title":"Editor theme","body":"` + preference + `"}`}},
			[][3]string{{"save-release", memory("save_memory"), `{"kind":"project","title":"Release day","body":"Releases ship on Friday"}`}},
		)
		for _, id := range []string{"save-theme", "save-release"} {
			if saved := result(id); !strings.Contains(saved, `"receipt"`) {
				t.Fatalf("%s result = %s", id, saved)
			}
		}
		// Capture both writes together; production may index between model
		// steps and legitimately discard an embedding from an older generation.
		close(firstBatch.ready)
		automatic := ready(t, "memory storage", 2)
		source := store.ReadKnowledgeSource(automatic.Definition.Source)
		owner, _ := dispatchPayload[protocol.KnowledgeOwnerPluginPayload](automatic.Owner.Data)
		if automatic.Owner.Kind != "plugin" || owner.Name != "memory" || source.Kind != "storage" || source.Scope != protocol.PluginStorageScopeNode || source.Prefix != "memory/entry/" {
			t.Fatalf("automatic collection = %+v", automatic)
		}
		if automatic.Progress.Model == nil || automatic.Progress.Chunks != 2 || encoded.Load() != 2 {
			t.Fatalf("progress = %+v after %d embedded texts", automatic.Progress, encoded.Load())
		}

		// The controller's knowledge search cites the plugin value that
		// holds the memory body.
		page, fault := query(t, "", "vector", protocol.KnowledgeModeHybrid)
		if fault != nil || len(page.Hits) == 0 || page.Mode != protocol.KnowledgeModeHybrid {
			t.Fatalf("controller search = %+v, %v", page, fault)
		}
		top := page.Hits[0]
		position, _ := dispatchPayload[protocol.KnowledgePositionValuePayload](top.Citation.Position.Data)
		if top.Citation.Collection != automatic.ID || !strings.HasPrefix(top.Citation.Path, "memory/entry/") || top.Citation.Position.Kind != "value" ||
			!strings.HasPrefix(position.Key, "memory/body/") || !strings.HasSuffix(position.Pointer, "/body") || top.Title != "Editor theme" || top.Text != preference {
			t.Fatalf("top hit = %+v", top)
		}
		cited, fault := cite(t, "", top.Citation)
		if fault != nil || cited.Text != preference || cited.Worktree != nil {
			t.Fatalf("citation = %+v, %v", cited, fault)
		}

		// Memory searches its own values by meaning: the query shares no
		// word with the entry.
		found := values(t, "memory", "向量")
		if found.Mode != protocol.KnowledgeModeSemantic || found.Indexing || len(found.Entries) == 0 || found.Entries[0].Key != top.Citation.Path {
			t.Fatalf("semantic value search = %+v", found)
		}
		// Its manifest declares no knowledge access, and other packages
		// see neither the collection nor the values.
		_, fault = query(t, "memory", "vector", protocol.KnowledgeModeHybrid)
		denied(t, fault, protocol.ErrorCodePermissionDenied, "")
		page, fault = query(t, reader, "vector", protocol.KnowledgeModeHybrid)
		if fault != nil || len(page.Hits) != 0 {
			t.Fatalf("another package searched memory: %+v, %v", page, fault)
		}
		_, fault = cite(t, reader, top.Citation)
		denied(t, fault, protocol.ErrorCodePermissionDenied, "not accessible")
		if foreign := values(t, reader, "vector"); len(foreign.Entries) != 0 {
			t.Fatalf("another package read memory values: %+v", foreign)
		}

		turn(t, [][3]string{{"search-before", memory("search_memory"), `{"query":"vector theme"}`}})
		before := result("search-before")
		id := regexp.MustCompile(`[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}`).FindString(before)
		if !strings.Contains(before, preference) || "memory/entry/"+id != top.Citation.Path {
			t.Fatalf("search_memory = %s", before)
		}
		turn(t,
			[][3]string{{"discover", "tool_search", `{"query":"select:` + memory("forget_memory") + `"}`}},
			[][3]string{{"forget", memory("forget_memory"), `{"id":"` + id + `","expected_revision":1}`}},
		)
		if forgotten := result("forget"); !strings.Contains(forgotten, `"removed"`) {
			t.Fatalf("forget_memory = %s", forgotten)
		}
		// Removal re-embeds nothing and takes the value out of every result.
		if remaining := ready(t, "memory storage", 1); remaining.Progress.Chunks != 1 || encoded.Load() != 2 {
			t.Fatalf("progress = %+v after %d embedded texts", remaining.Progress, encoded.Load())
		}
		if page, fault := query(t, "", "vector", protocol.KnowledgeModeHybrid); fault != nil || hit(page, top.Citation.Path) != nil {
			t.Fatalf("removed memory is still found: %+v, %v", page, fault)
		}
		if _, fault := cite(t, "", top.Citation); fault == nil {
			t.Fatal("removed memory is still cited")
		}
		if found := values(t, "memory", "向量"); slices.ContainsFunc(found.Entries, func(entry protocol.PluginStorageEntry) bool { return entry.Key == top.Citation.Path }) {
			t.Fatalf("removed memory is still a value: %+v", found)
		}
		turn(t, [][3]string{{"search-after", memory("search_memory"), `{"query":"vector theme"}`}})
		if after := result("search-after"); after == "" || strings.Contains(after, preference) {
			t.Fatalf("search_memory after removal = %s", after)
		}
	}) {
		return
	}

	if !t.Run("storage collections are scoped to their owner", func(t *testing.T) {
		_, fault := call(t, "", "create_knowledge_collection", storage(protocol.PluginStorageScopeNode, "Ownerless", "note/"))
		denied(t, fault, protocol.ErrorCodePermissionDenied, "real plugin owner")
		_, fault = call(t, blind, "create_knowledge_collection", storage(protocol.PluginStorageScopeNode, "Blind", "note/"))
		denied(t, fault, protocol.ErrorCodePermissionDenied, "captured storage access")
		_, fault = call(t, notes, "create_knowledge_collection", storage(protocol.PluginStorageScopeConversation, "Conversation", "note/"))
		denied(t, fault, protocol.ErrorCodePermissionDenied, "captured session")
		filtered := storage(protocol.PluginStorageScopeNode, "Filtered", "note/")
		filtered.Definition.Globs = []string{"*.md"}
		_, fault = call(t, notes, "create_knowledge_collection", filtered)
		denied(t, fault, protocol.ErrorCodeInvalidRequest, "")
		if collection(t, "Ownerless") != nil || collection(t, "Blind") != nil || collection(t, "Conversation") != nil || collection(t, "Filtered") != nil {
			t.Fatal("a rejected storage collection was registered")
		}

		batch := &catalogBarrier{ready: make(chan struct{}), waiting: make(chan struct{})}
		catalogGate.Store(batch)
		node.knowledge.Invalidate()
		eventually(t, "catalog capture barrier", func() bool {
			select {
			case <-batch.waiting:
				return true
			default:
				return false
			}
		})
		baseline := encoded.Load()
		must(t, notes, "create_knowledge_collection", storage(protocol.PluginStorageScopeNode, "Notes", "note/"))
		write := func(key, value string, revision uint64) {
			t.Helper()
			must(t, notes, "write_plugin_value", &protocol.CommandWritePluginValuePayload{Key: key, Value: json.RawMessage(value), ExpectedRevision: revision})
		}
		write("note/a", `"alpha vector text"`, 0)
		write("note/b", `{"title":"Beta","text":"beta body"}`, 0)
		write("other/c", `"outside the prefix"`, 0)
		close(batch.ready)
		if owned := ready(t, "Notes", 2); owned.Progress.Chunks != 2 || encoded.Load() != baseline+2 {
			t.Fatalf("progress = %+v after %d embedded texts", owned.Progress, encoded.Load()-baseline)
		}
		page, fault := query(t, notes, "alpha", protocol.KnowledgeModeKeyword)
		alpha := hit(page, "note/a")
		if fault != nil || alpha == nil || alpha.Citation.Position.Kind != "value" || alpha.Title != "note/a" {
			t.Fatalf("owner search = %+v, %v", page, fault)
		}
		if cited, fault := cite(t, notes, alpha.Citation); fault != nil || cited.Text != "alpha vector text" {
			t.Fatalf("citation = %+v, %v", cited, fault)
		}
		if page, fault := query(t, notes, "beta", protocol.KnowledgeModeKeyword); fault != nil || hit(page, "note/b") == nil || hit(page, "note/b").Title != "Beta" {
			t.Fatalf("object value search = %+v, %v", page, fault)
		}
		if page, fault := query(t, notes, "outside", protocol.KnowledgeModeKeyword); fault != nil || len(page.Hits) != 0 {
			t.Fatalf("value outside the prefix was indexed: %+v, %v", page, fault)
		}
		if page, fault := query(t, reader, "alpha", protocol.KnowledgeModeKeyword); fault != nil || len(page.Hits) != 0 {
			t.Fatalf("another package searched the notes: %+v, %v", page, fault)
		}
		_, fault = cite(t, reader, alpha.Citation)
		denied(t, fault, protocol.ErrorCodePermissionDenied, "not accessible")

		// A controller registers a storage collection for a named package.
		selected := storage(protocol.PluginStorageScopeNode, "Other notes", "other/")
		owner := notes
		selected.Plugin = &owner
		must(t, "", "create_knowledge_collection", selected)
		ready(t, "Other notes", 1)
		if encoded.Load() != baseline+3 {
			t.Fatalf("%d texts embedded for one new value", encoded.Load()-baseline-2)
		}

		// A conversation collection indexes the values of its own
		// conversation and is read only there.
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionFull
		session, _ := dispatchPayload[protocol.Session](must(t, "", "create_session", protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}).Data)
		conversation, talk = &session, session
		must(t, notes, "create_knowledge_collection", storage(protocol.PluginStorageScopeConversation, "Conversation notes", "talk/"))
		must(t, notes, "write_plugin_conversation_value", &protocol.CommandWritePluginConversationValuePayload{Key: "talk/a", Value: json.RawMessage(`"epsilon remark"`)})
		ready(t, "Conversation notes", 1)
		page, fault = query(t, notes, "epsilon", protocol.KnowledgeModeKeyword)
		remark := hit(page, "talk/a")
		if fault != nil || remark == nil {
			t.Fatalf("conversation search = %+v, %v", page, fault)
		}
		if cited, fault := cite(t, notes, remark.Citation); fault != nil || cited.Text != "epsilon remark" {
			t.Fatalf("conversation citation = %+v, %v", cited, fault)
		}
		conversation = nil
		if page, fault := query(t, notes, "epsilon", protocol.KnowledgeModeKeyword); fault != nil || len(page.Hits) != 0 {
			t.Fatalf("conversation values left their conversation: %+v, %v", page, fault)
		}
		if encoded.Load() != baseline+4 {
			t.Fatalf("%d texts embedded for one conversation value", encoded.Load()-baseline-3)
		}
		baseline++

		// Only the changed value is embedded again.
		write("note/b", `{"title":"Beta","text":"gamma body"}`, 1)
		eventually(t, "changed note indexing", func() bool {
			page, fault := query(t, notes, "gamma", protocol.KnowledgeModeKeyword)
			return fault == nil && hit(page, "note/b") != nil
		})
		if encoded.Load() != baseline+4 {
			t.Fatalf("%d texts embedded for one changed value", encoded.Load()-baseline-3)
		}
		must(t, notes, "remove_plugin_value", &protocol.CommandRemovePluginValuePayload{Key: "note/a", ExpectedRevision: 1})
		eventually(t, "removed note pruning", func() bool {
			page, fault := query(t, notes, "alpha", protocol.KnowledgeModeKeyword)
			return fault == nil && len(page.Hits) == 0
		})
		if _, fault := cite(t, notes, alpha.Citation); fault == nil {
			t.Fatal("removed note is still cited")
		}
		if owned := ready(t, "Notes", 1); encoded.Load() != baseline+4 {
			t.Fatalf("progress = %+v after %d embedded texts", owned.Progress, encoded.Load()-baseline)
		}

		// A value written while indexing is down is indexed after a
		// restart, and nothing indexed before is embedded again.
		node.knowledge.Close()
		write("note/d", `"delta text"`, 0)
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		start(t)
		eventually(t, "restart indexing", func() bool {
			page, fault := query(t, notes, "delta", protocol.KnowledgeModeKeyword)
			return fault == nil && hit(page, "note/d") != nil
		})
		ready(t, "Notes", 2)
		ready(t, "Other notes", 1)
		ready(t, "memory storage", 1)
		if encoded.Load() != baseline+5 {
			t.Fatalf("%d texts embedded after the restart", encoded.Load()-baseline-4)
		}
		if page, fault := query(t, notes, "gamma", protocol.KnowledgeModeKeyword); fault != nil || hit(page, "note/b") == nil {
			t.Fatalf("search after restart = %+v, %v", page, fault)
		}
		if found := values(t, "memory", "Friday"); len(found.Entries) != 1 {
			t.Fatalf("memory values after restart = %+v", found)
		}
	}) {
		return
	}

	t.Run("collections leave with their owner", func(t *testing.T) {
		if err := os.WriteFile(filepath.Join(root, "guide.md"), []byte("# Guide\n\nzeta handbook\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		must(t, "", "create_knowledge_collection", &protocol.CommandCreateKnowledgeCollectionPayload{Definition: protocol.KnowledgeDefinition{
			Name: "Project files", Globs: []string{"*.md"}, Tags: []string{}, SharedWith: []string{},
			Source: protocol.KnowledgeSource{Kind: "worktree", Data: &protocol.KnowledgeSourceWorktreePayload{Worktree: talk.Worktree, Path: ""}},
		}})
		ready(t, "Project files", 1)
		ready(t, "Conversation notes", 1)
		indexed := func() (ids []string) {
			t.Helper()
			if err := node.profile.Store.Knowledge.Read(t.Context(), func(db *sql.Conn) error {
				rows, err := db.QueryContext(t.Context(), "SELECT id FROM collections ORDER BY id")
				if err != nil {
					return err
				}
				defer rows.Close()
				for rows.Next() {
					var id string
					if err := rows.Scan(&id); err != nil {
						return err
					}
					ids = append(ids, id)
				}
				return rows.Err()
			}); err != nil {
				t.Fatal(err)
			}
			return ids
		}
		// Capture the collection identities before removing their owner, then
		// wait until both the collections and their derived index are removed.
		removal := func(names ...string) func() {
			t.Helper()
			var ids []string
			for _, name := range names {
				current := collection(t, name)
				if current == nil {
					t.Fatalf("%s does not exist before its owner is removed", name)
				}
				ids = append(ids, current.ID.String())
			}
			return func() {
				t.Helper()
				eventually(t, strings.Join(names, ", ")+" removal", func() bool {
					for _, name := range names {
						if collection(t, name) != nil {
							return false
						}
					}
					left := indexed()
					return !slices.ContainsFunc(ids, func(id string) bool { return slices.Contains(left, id) })
				})
			}
		}

		revision := func() uint64 {
			t.Helper()
			var installed *protocol.PluginInfo
			if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) (err error) {
				installed, err = store.ReadPlugin(t.Context(), db, notes)
				return err
			}); err != nil || installed == nil {
				t.Fatalf("notes package = %+v, %v", installed, err)
			}
			return installed.Summary.Revision
		}

		// A disabled package still owns its collections: the pass that
		// removes the conversation's collection keeps the others.
		conversationRemoved := removal("Conversation notes")
		must(t, "", "set_plugin_enabled", &protocol.CommandSetPluginEnabledPayload{Name: notes, Enabled: false, ExpectedRevision: revision()})
		must(t, "", "remove_session", &protocol.CommandRemoveSessionPayload{Session: talk.ID, ExpectedRevision: talk.Revision})
		conversationRemoved()
		if collection(t, "Notes") == nil || collection(t, "Other notes") == nil || collection(t, "Project files") == nil {
			t.Fatal("removing a conversation removed a collection it does not own")
		}

		projectRemoved := removal("Project files")
		must(t, "", "remove_project", &protocol.CommandRemoveProjectPayload{Expected: project})
		projectRemoved()

		notesRemoved := removal("Notes", "Other notes")
		must(t, "", "remove_plugin", &protocol.CommandRemovePluginPayload{Name: notes, ExpectedRevision: revision()})
		notesRemoved()
		if collection(t, "memory storage") == nil {
			t.Fatal("removing a package removed another package's collection")
		}
	})
}
