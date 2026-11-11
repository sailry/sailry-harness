package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// pluginStorageScript discovers the note tools, saves one note, then reads the
// same key through the owning package and through another package.
type pluginStorageScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	offered [][]string
	results map[string]string
}

func (script *pluginStorageScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	var names []string
	for _, tool := range call.Tools {
		names = append(names, tool.GetName())
	}
	script.offered = append(script.offered, names)
	step := len(script.offered)
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found {
				if text, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output); found {
					script.results[result.ToolCallID] = text.Text
				}
			}
		}
	}
	script.mu.Unlock()
	note := func(name string) string { return operations.Alias("task-notes", name) }
	peek := operations.Alias("storage-peek-fixture", "peek")
	var calls [][3]string
	switch step {
	case 1:
		calls = [][3]string{{"discover", "tool_search", `{"query":"select:` + strings.Join([]string{note("read_note"), note("list_notes"), note("save_note"), note("delete_note"), peek}, ",") + `"}`}}
	case 2:
		calls = [][3]string{{"save", note("save_note"), `{"key":"todo","value":{"text":"buy milk"},"expected_revision":0}`}}
	case 3:
		calls = [][3]string{{"read", note("read_note"), `{"key":"todo"}`}, {"peek", peek, `{"key":"todo"}`}, {"list", note("list_notes"), `{"prefix":"","after":null,"limit":500}`},
			{"progress", note("progress"), `{"title":null,"steps":[{"description":"Save the note","state":"completed"}]}`}}
	case 4:
	default:
		return nil, errors.New("unexpected extra storage model request")
	}
	return func(yield func(fantasy.StreamPart) bool) {
		for _, call := range calls {
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: call[0], ToolCallName: call[1], ToolCallInput: call[2]}) {
				return
			}
		}
		if len(calls) == 0 && !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Saved the note"}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

// TestPluginStorageTools runs a real turn in which the model calls the
// task-notes example's declared storage and progress tools.
func TestPluginStorageTools(t *testing.T) {
	script := &pluginStorageScript{results: map[string]string{}}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("storage tools must not launch a process")
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	other := t.TempDir()
	if err := os.WriteFile(filepath.Join(other, "plugin.json"), []byte(`{"$schema":"`+plugins.ManifestSchema+`","name":"storage-peek-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read"],"tools":[{"name":"peek","operation":"storage.get"}]}}}`), 0o600); err != nil {
		t.Fatal(err)
	}
	references := map[string]protocol.PluginReference{}
	for name, source := range map[string]string{"task-notes": filepath.Join("..", "..", "plugins", "examples", "task-notes"), "storage-peek-fixture": other} {
		info, err := node.plugins.manager.Install(t.Context(), source, "", name)
		if err != nil || len(info.Issues) != 0 || info.Extension == nil {
			t.Fatalf("%s is invalid: %+v, %v", name, info.Issues, err)
		}
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			var err error
			info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		references[name] = plugins.PackageReference(info)
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
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Storage tools", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Save my note"},
	}})
	var history protocol.ConversationHistory
	for completed := false; !completed; {
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
				completed = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("storage turn failed: %+v", run)
			}
		}
		if !completed {
			if _, err := subscription.Next(t.Context()); err != nil {
				t.Fatal(err)
			}
		}
	}
	script.mu.Lock()
	offered, results := script.offered, script.results
	script.mu.Unlock()
	if len(offered) != 4 {
		t.Fatalf("storage turn made %d model requests", len(offered))
	}
	notes := []string{"read_note", "list_notes", "save_note", "delete_note"}
	if !slices.Contains(offered[0], operations.Alias("task-notes", "progress")) || slices.ContainsFunc(notes, func(name string) bool {
		return slices.Contains(offered[0], operations.Alias("task-notes", name))
	}) {
		t.Fatalf("first request must offer progress and defer storage: %v", offered[0])
	}
	for _, name := range append(notes, "progress") {
		if !slices.Contains(offered[1], operations.Alias("task-notes", name)) {
			t.Fatalf("task-notes %s was not offered after discovery: %v", name, offered[1])
		}
	}
	for call, expected := range map[string][]string{
		"save":     {`"key":"todo"`, `"revision":1`},
		"read":     {`"text":"buy milk"`, `"revision":1`, `"present":true`},
		"peek":     {`"present":false`},
		"list":     {`"keys":["todo"]`},
		"progress": {`"progress":{`, `"description":"Save the note"`},
	} {
		for _, fragment := range expected {
			if !strings.Contains(results[call], fragment) {
				t.Fatalf("%s result %q omits %s", call, results[call], fragment)
			}
		}
	}
	// The installed package's declared presentation is recorded with the call
	// and projected with its progress snapshot.
	body, err := json.Marshal(history)
	if err != nil {
		t.Fatal(err)
	}
	var projected any
	if err := json.Unmarshal(body, &projected); err != nil {
		t.Fatal(err)
	}
	var calls []map[string]any
	var walk func(any)
	walk = func(value any) {
		switch value := value.(type) {
		case map[string]any:
			if _, call := value["presentation"]; call {
				calls = append(calls, value)
			}
			for _, child := range value {
				walk(child)
			}
		case []any:
			for _, child := range value {
				walk(child)
			}
		}
	}
	walk(projected)
	presentations := map[string]any{}
	for _, call := range calls {
		name, _ := call["name"].(string)
		presentations[name] = call["presentation"]
	}
	if !strings.Contains(string(body), `"progress":{"title":null,"steps":[{"description":"Save the note","state":"completed"}]}`) {
		t.Fatal("progress result was not projected as a progress snapshot")
	}
	if presentations[operations.Alias("task-notes", "progress")] != "progress" || presentations[operations.Alias("task-notes", "save_note")] != "summary" {
		t.Fatalf("installed tool presentations were not recorded: %v", presentations)
	}
	if len(history.Page.Approvals) != 1 || history.Page.Approvals[0].Source != protocol.ConversationApprovalSourceStorage || history.Page.Approvals[0].State != protocol.ConversationApprovalStateApproved {
		t.Fatalf("note write was not authorized by its storage scope: %+v", history.Page.Approvals)
	}
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		owned, err := store.ReadPluginValue(t.Context(), db, references["task-notes"], "todo")
		if err != nil {
			return err
		}
		foreign, err := store.ReadPluginValue(t.Context(), db, references["storage-peek-fixture"], "todo")
		if err != nil {
			return err
		}
		if !owned.Present || owned.Revision != 1 || !strings.Contains(string(owned.Value), "buy milk") || foreign.Present {
			t.Fatalf("note left its package scope: %+v, %+v", owned, foreign)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
