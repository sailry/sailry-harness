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

// TestKnowledgeStorage indexes plugin storage through a real Node with a fake
// embedding model: the bundled memory package's own tools feed its automatic
// semantic collection, and a fixture package owns an explicit storage
// collection.
func TestKnowledgeStorage(t *testing.T) {
	script := &knowledgeStorageScript{results: map[string]string{}}
	var encoded atomic.Int64
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	profile := filepath.Join(t.TempDir(), "profile")
	var node *Node
	start := func() {
		t.Helper()
		started, err := StartWithOptions(t.Context(), profile, Options{
			Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
			Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
				return protocol.ProcessCompletion{}, errors.New("storage knowledge must not launch a process")
			}},
			// Texts about vectors lie on one axis and all others on the other.
			knowledge: func(options *knowledgeservice.Options) {
				selected := localmodels.DefaultEmbedding
				options.Choice = func() protocol.ModelsLocalChoice { return protocol.ModelsLocalChoice{Enabled: true, Model: &selected} }
				options.Dimension = func(model string) (int, bool) { return 2, model == selected }
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
	start()
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
	call := func(owner string, kind string, data any) (protocol.Output, *protocol.Fault) {
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
	must := func(owner string, kind string, data any) protocol.Output {
		t.Helper()
		output, fault := call(owner, kind, data)
		if fault != nil {
			t.Fatalf("%s as %q failed: %v", kind, owner, fault)
		}
		return output
	}
	denied := func(fault *protocol.Fault, code protocol.ErrorCode, fragment string) {
		t.Helper()
		if fault == nil || fault.Code != code || !strings.Contains(fault.Message, fragment) {
			t.Fatalf("fault = %v, want %s containing %q", fault, code, fragment)
		}
	}
	eventually := func(what string, done func() bool) {
		t.Helper()
		for deadline := time.Now().Add(30 * time.Second); !done(); time.Sleep(20 * time.Millisecond) {
			if time.Now().After(deadline) {
				t.Fatalf("%s did not happen", what)
			}
		}
	}
	collection := func(name string) *protocol.KnowledgeCollection {
		t.Helper()
		listed, _ := dispatchPayload[[]protocol.KnowledgeCollection](must("", "list_knowledge_collections", nil).Data)
		for _, collection := range listed {
			if collection.Definition.Name == name {
				return &collection
			}
		}
		return nil
	}
	ready := func(name string, sources uint64) protocol.KnowledgeCollection {
		t.Helper()
		var found protocol.KnowledgeCollection
		eventually(name+" indexing", func() bool {
			current := collection(name)
			if current == nil || current.Progress.Status != protocol.KnowledgeStatusReady || current.Progress.Sources != sources {
				return false
			}
			found = *current
			return true
		})
		return found
	}
	query := func(owner, text string, mode protocol.KnowledgeMode) (protocol.KnowledgePage, *protocol.Fault) {
		t.Helper()
		output, fault := call(owner, "query_knowledge", &protocol.KnowledgeQuery{Query: text, Collections: []protocol.KnowledgeID{}, Mode: mode, Tags: []string{}, Limit: 5})
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
	cite := func(owner string, citation protocol.KnowledgeCitation) (protocol.KnowledgeCitationPage, *protocol.Fault) {
		t.Helper()
		output, fault := call(owner, "read_knowledge_citation", &protocol.KnowledgeCitationRequest{Collection: citation.Collection, Chunk: citation.Chunk, Revision: citation.Revision, Limit: 4096})
		page, _ := dispatchPayload[protocol.KnowledgeCitationPage](output.Data)
		return page, fault
	}
	values := func(owner string, text string) protocol.PluginStorageSearchPage {
		t.Helper()
		search := protocol.DefaultPluginStorageSearch()
		search.Query, search.Weights, search.Limit = &text, [2]uint16{3, 1}, 10
		page, _ := dispatchPayload[protocol.PluginStorageSearchPage](must(owner, "search_plugin_values", &search).Data)
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
	project, _ := dispatchPayload[protocol.Project](must("", "create_project", protocol.ProjectsDraft{
		Name: "Storage knowledge", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}).Data)
	// turn runs one conversation turn in which the model makes the planned
	// tool calls, one list per model request.
	turn := func(steps ...[][3]string) {
		t.Helper()
		script.mu.Lock()
		script.steps = steps
		script.mu.Unlock()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionFull
		session, _ := dispatchPayload[protocol.Session](must("", "create_session", protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}).Data)
		must("", "submit_turn", protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Use memory"}})
		eventually("memory turn", func() bool {
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

	t.Run("memory tools feed an automatic collection", func(t *testing.T) {
		if collection("memory storage") != nil {
			t.Fatal("automatic collection exists before its first indexed value")
		}
		turn(
			[][3]string{{"save-theme", memory("save_memory"), `{"kind":"user","title":"Editor theme","body":"` + preference + `"}`}},
			[][3]string{{"save-release", memory("save_memory"), `{"kind":"project","title":"Release day","body":"Releases ship on Friday"}`}},
		)
		for _, id := range []string{"save-theme", "save-release"} {
			if saved := result(id); !strings.Contains(saved, `"receipt"`) {
				t.Fatalf("%s result = %s", id, saved)
			}
		}
		automatic := ready("memory storage", 2)
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
		page, fault := query("", "vector", protocol.KnowledgeModeHybrid)
		if fault != nil || len(page.Hits) == 0 || page.Mode != protocol.KnowledgeModeHybrid {
			t.Fatalf("controller search = %+v, %v", page, fault)
		}
		top := page.Hits[0]
		position, _ := dispatchPayload[protocol.KnowledgePositionValuePayload](top.Citation.Position.Data)
		if top.Citation.Collection != automatic.ID || !strings.HasPrefix(top.Citation.Path, "memory/entry/") || top.Citation.Position.Kind != "value" ||
			!strings.HasPrefix(position.Key, "memory/body/") || !strings.HasSuffix(position.Pointer, "/body") || top.Title != "Editor theme" || top.Text != preference {
			t.Fatalf("top hit = %+v", top)
		}
		cited, fault := cite("", top.Citation)
		if fault != nil || cited.Text != preference || cited.Worktree != nil {
			t.Fatalf("citation = %+v, %v", cited, fault)
		}

		// Memory searches its own values by meaning: the query shares no
		// word with the entry.
		found := values("memory", "向量")
		if found.Mode != protocol.KnowledgeModeSemantic || found.Indexing || len(found.Entries) == 0 || found.Entries[0].Key != top.Citation.Path {
			t.Fatalf("semantic value search = %+v", found)
		}
		// Its manifest declares no knowledge access, and other packages
		// see neither the collection nor the values.
		_, fault = query("memory", "vector", protocol.KnowledgeModeHybrid)
		denied(fault, protocol.ErrorCodePermissionDenied, "")
		page, fault = query(reader, "vector", protocol.KnowledgeModeHybrid)
		if fault != nil || len(page.Hits) != 0 {
			t.Fatalf("another package searched memory: %+v, %v", page, fault)
		}
		_, fault = cite(reader, top.Citation)
		denied(fault, protocol.ErrorCodePermissionDenied, "not accessible")
		if foreign := values(reader, "vector"); len(foreign.Entries) != 0 {
			t.Fatalf("another package read memory values: %+v", foreign)
		}

		turn([][3]string{{"search-before", memory("search_memory"), `{"query":"vector theme"}`}})
		before := result("search-before")
		id := regexp.MustCompile(`[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}`).FindString(before)
		if !strings.Contains(before, preference) || "memory/entry/"+id != top.Citation.Path {
			t.Fatalf("search_memory = %s", before)
		}
		turn(
			[][3]string{{"discover", "tool_search", `{"query":"select:` + memory("forget_memory") + `"}`}},
			[][3]string{{"forget", memory("forget_memory"), `{"id":"` + id + `","expected_revision":1}`}},
		)
		if forgotten := result("forget"); !strings.Contains(forgotten, `"removed"`) {
			t.Fatalf("forget_memory = %s", forgotten)
		}
		// Removal re-embeds nothing and takes the value out of every result.
		if remaining := ready("memory storage", 1); remaining.Progress.Chunks != 1 || encoded.Load() != 2 {
			t.Fatalf("progress = %+v after %d embedded texts", remaining.Progress, encoded.Load())
		}
		if page, fault := query("", "vector", protocol.KnowledgeModeHybrid); fault != nil || hit(page, top.Citation.Path) != nil {
			t.Fatalf("removed memory is still found: %+v, %v", page, fault)
		}
		if _, fault := cite("", top.Citation); fault == nil {
			t.Fatal("removed memory is still cited")
		}
		if found := values("memory", "向量"); slices.ContainsFunc(found.Entries, func(entry protocol.PluginStorageEntry) bool { return entry.Key == top.Citation.Path }) {
			t.Fatalf("removed memory is still a value: %+v", found)
		}
		turn([][3]string{{"search-after", memory("search_memory"), `{"query":"vector theme"}`}})
		if after := result("search-after"); after == "" || strings.Contains(after, preference) {
			t.Fatalf("search_memory after removal = %s", after)
		}
	})

	t.Run("storage collections are scoped to their owner", func(t *testing.T) {
		_, fault := call("", "create_knowledge_collection", storage(protocol.PluginStorageScopeNode, "Ownerless", "note/"))
		denied(fault, protocol.ErrorCodePermissionDenied, "real plugin owner")
		_, fault = call(blind, "create_knowledge_collection", storage(protocol.PluginStorageScopeNode, "Blind", "note/"))
		denied(fault, protocol.ErrorCodePermissionDenied, "captured storage access")
		_, fault = call(notes, "create_knowledge_collection", storage(protocol.PluginStorageScopeConversation, "Conversation", "note/"))
		denied(fault, protocol.ErrorCodePermissionDenied, "captured session")
		filtered := storage(protocol.PluginStorageScopeNode, "Filtered", "note/")
		filtered.Definition.Globs = []string{"*.md"}
		_, fault = call(notes, "create_knowledge_collection", filtered)
		denied(fault, protocol.ErrorCodeInvalidRequest, "")
		if collection("Ownerless") != nil || collection("Blind") != nil || collection("Conversation") != nil || collection("Filtered") != nil {
			t.Fatal("a rejected storage collection was registered")
		}

		baseline := encoded.Load()
		must(notes, "create_knowledge_collection", storage(protocol.PluginStorageScopeNode, "Notes", "note/"))
		write := func(key, value string, revision uint64) {
			t.Helper()
			must(notes, "write_plugin_value", &protocol.CommandWritePluginValuePayload{Key: key, Value: json.RawMessage(value), ExpectedRevision: revision})
		}
		write("note/a", `"alpha vector text"`, 0)
		write("note/b", `{"title":"Beta","text":"beta body"}`, 0)
		write("other/c", `"outside the prefix"`, 0)
		if owned := ready("Notes", 2); owned.Progress.Chunks != 2 || encoded.Load() != baseline+2 {
			t.Fatalf("progress = %+v after %d embedded texts", owned.Progress, encoded.Load()-baseline)
		}
		page, fault := query(notes, "alpha", protocol.KnowledgeModeKeyword)
		alpha := hit(page, "note/a")
		if fault != nil || alpha == nil || alpha.Citation.Position.Kind != "value" || alpha.Title != "note/a" {
			t.Fatalf("owner search = %+v, %v", page, fault)
		}
		if cited, fault := cite(notes, alpha.Citation); fault != nil || cited.Text != "alpha vector text" {
			t.Fatalf("citation = %+v, %v", cited, fault)
		}
		if page, fault := query(notes, "beta", protocol.KnowledgeModeKeyword); fault != nil || hit(page, "note/b") == nil || hit(page, "note/b").Title != "Beta" {
			t.Fatalf("object value search = %+v, %v", page, fault)
		}
		if page, fault := query(notes, "outside", protocol.KnowledgeModeKeyword); fault != nil || len(page.Hits) != 0 {
			t.Fatalf("value outside the prefix was indexed: %+v, %v", page, fault)
		}
		if page, fault := query(reader, "alpha", protocol.KnowledgeModeKeyword); fault != nil || len(page.Hits) != 0 {
			t.Fatalf("another package searched the notes: %+v, %v", page, fault)
		}
		_, fault = cite(reader, alpha.Citation)
		denied(fault, protocol.ErrorCodePermissionDenied, "not accessible")

		// A controller registers a storage collection for a named package.
		selected := storage(protocol.PluginStorageScopeNode, "Other notes", "other/")
		owner := notes
		selected.Plugin = &owner
		must("", "create_knowledge_collection", selected)
		ready("Other notes", 1)
		if encoded.Load() != baseline+3 {
			t.Fatalf("%d texts embedded for one new value", encoded.Load()-baseline-2)
		}

		// A conversation collection indexes the values of its own
		// conversation and is read only there.
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionFull
		session, _ := dispatchPayload[protocol.Session](must("", "create_session", protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}).Data)
		conversation, talk = &session, session
		must(notes, "create_knowledge_collection", storage(protocol.PluginStorageScopeConversation, "Conversation notes", "talk/"))
		must(notes, "write_plugin_conversation_value", &protocol.CommandWritePluginConversationValuePayload{Key: "talk/a", Value: json.RawMessage(`"epsilon remark"`)})
		ready("Conversation notes", 1)
		page, fault = query(notes, "epsilon", protocol.KnowledgeModeKeyword)
		remark := hit(page, "talk/a")
		if fault != nil || remark == nil {
			t.Fatalf("conversation search = %+v, %v", page, fault)
		}
		if cited, fault := cite(notes, remark.Citation); fault != nil || cited.Text != "epsilon remark" {
			t.Fatalf("conversation citation = %+v, %v", cited, fault)
		}
		conversation = nil
		if page, fault := query(notes, "epsilon", protocol.KnowledgeModeKeyword); fault != nil || len(page.Hits) != 0 {
			t.Fatalf("conversation values left their conversation: %+v, %v", page, fault)
		}
		if encoded.Load() != baseline+4 {
			t.Fatalf("%d texts embedded for one conversation value", encoded.Load()-baseline-3)
		}
		baseline++

		// Only the changed value is embedded again.
		write("note/b", `{"title":"Beta","text":"gamma body"}`, 1)
		eventually("changed note indexing", func() bool {
			page, fault := query(notes, "gamma", protocol.KnowledgeModeKeyword)
			return fault == nil && hit(page, "note/b") != nil
		})
		if encoded.Load() != baseline+4 {
			t.Fatalf("%d texts embedded for one changed value", encoded.Load()-baseline-3)
		}
		must(notes, "remove_plugin_value", &protocol.CommandRemovePluginValuePayload{Key: "note/a", ExpectedRevision: 1})
		eventually("removed note pruning", func() bool {
			page, fault := query(notes, "alpha", protocol.KnowledgeModeKeyword)
			return fault == nil && len(page.Hits) == 0
		})
		if _, fault := cite(notes, alpha.Citation); fault == nil {
			t.Fatal("removed note is still cited")
		}
		if owned := ready("Notes", 1); encoded.Load() != baseline+4 {
			t.Fatalf("progress = %+v after %d embedded texts", owned.Progress, encoded.Load()-baseline)
		}

		// A value written while indexing is down is indexed after a
		// restart, and nothing indexed before is embedded again.
		node.knowledge.Close()
		write("note/d", `"delta text"`, 0)
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		start()
		eventually("restart indexing", func() bool {
			page, fault := query(notes, "delta", protocol.KnowledgeModeKeyword)
			return fault == nil && hit(page, "note/d") != nil
		})
		ready("Notes", 2)
		ready("Other notes", 1)
		ready("memory storage", 1)
		if encoded.Load() != baseline+5 {
			t.Fatalf("%d texts embedded after the restart", encoded.Load()-baseline-4)
		}
		if page, fault := query(notes, "gamma", protocol.KnowledgeModeKeyword); fault != nil || hit(page, "note/b") == nil {
			t.Fatalf("search after restart = %+v, %v", page, fault)
		}
		if found := values("memory", "Friday"); len(found.Entries) != 1 {
			t.Fatalf("memory values after restart = %+v", found)
		}
	})

	t.Run("collections leave with their owner", func(t *testing.T) {
		if err := os.WriteFile(filepath.Join(root, "guide.md"), []byte("# Guide\n\nzeta handbook\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		must("", "create_knowledge_collection", &protocol.CommandCreateKnowledgeCollectionPayload{Definition: protocol.KnowledgeDefinition{
			Name: "Project files", Globs: []string{"*.md"}, Tags: []string{}, SharedWith: []string{},
			Source: protocol.KnowledgeSource{Kind: "worktree", Data: &protocol.KnowledgeSourceWorktreePayload{Worktree: talk.Worktree, Path: ""}},
		}})
		ready("Project files", 1)
		ready("Conversation notes", 1)
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
		// gone waits until the collection and its derived index are removed.
		gone := func(names ...string) {
			t.Helper()
			var ids []string
			for _, name := range names {
				current := collection(name)
				if current == nil {
					t.Fatalf("%s does not exist before its owner is removed", name)
				}
				ids = append(ids, current.ID.String())
			}
			eventually(strings.Join(names, ", ")+" removal", func() bool {
				for _, name := range names {
					if collection(name) != nil {
						return false
					}
				}
				left := indexed()
				return !slices.ContainsFunc(ids, func(id string) bool { return slices.Contains(left, id) })
			})
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
		must("", "set_plugin_enabled", &protocol.CommandSetPluginEnabledPayload{Name: notes, Enabled: false, ExpectedRevision: revision()})
		must("", "remove_session", &protocol.CommandRemoveSessionPayload{Session: talk.ID, ExpectedRevision: talk.Revision})
		gone("Conversation notes")
		if collection("Notes") == nil || collection("Other notes") == nil || collection("Project files") == nil {
			t.Fatal("removing a conversation removed a collection it does not own")
		}

		must("", "remove_project", &protocol.CommandRemoveProjectPayload{Expected: project})
		gone("Project files")

		must("", "remove_plugin", &protocol.CommandRemovePluginPayload{Name: notes, ExpectedRevision: revision()})
		gone("Notes", "Other notes")
		if collection("memory storage") == nil {
			t.Fatal("removing a package removed another package's collection")
		}
	})
}
