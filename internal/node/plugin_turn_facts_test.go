package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
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

// turnFactsModel discovers and calls the fixture's tool in the user's turn,
// then answers; it answers the continuation at once. Every request reports
// usage and keeps the context text it was sent.
type turnFactsModel struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	prompts []string
}

func (script *turnFactsModel) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	var text strings.Builder
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if value, found := fantasy.AsMessagePart[fantasy.TextPart](part); found {
				text.WriteString(value.Text)
				text.WriteByte('\n')
			}
		}
	}
	script.mu.Lock()
	script.prompts = append(script.prompts, text.String())
	step := len(script.prompts)
	script.mu.Unlock()
	note := operations.Alias("turn-facts-fixture", "note")
	var calls [][3]string
	var input, output int64
	switch step {
	case 1:
		calls, input, output = [][3]string{{"discover", "tool_search", `{"query":"select:` + note + `"}`}}, 100, 20
	case 2:
		calls, input, output = [][3]string{{"note", note, `{}`}}, 120, 10
	case 3:
		input, output = 150, 5
	case 4:
		input, output = 7, 3
	default:
		return nil, errors.New("unexpected extra model request")
	}
	return func(yield func(fantasy.StreamPart) bool) {
		for _, call := range calls {
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: call[0], ToolCallName: call[1], ToolCallInput: call[2]}) {
				return
			}
		}
		if len(calls) == 0 && !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop,
			ProviderMetadata: fantasy.ProviderMetadata{model.UsageMetadataKey: &model.UsageMetadata{Reported: true, Input: &input, Output: &output}}})
	}, nil
}

// TestPluginTurnFacts runs a user turn and the continuation its package
// starts, and checks the facts both callbacks receive: what started the turn,
// the tools the model called, the turn's and the session's usage, and the
// automatic chain with the limit the continuation declared.
func TestPluginTurnFacts(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	script := &turnFactsModel{}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	source := t.TempDir()
	entry := "dev.sailry.platform/host/main.js"
	for name, content := range map[string]string{
		"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"turn-facts-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read","storage.write","conversation.read","conversation.control"],` +
			`"tools":[{"name":"note","presentation":"content","description":"Record a note.","handler":{"name":"note","read_only":true,"parameters":{"type":"object","additionalProperties":false,"properties":{},"required":[]}}}],` +
			`"continuation_marker":{"label":"Continue work"},"continuation_markers":{"verify":{"label":"Verify work"}},` +
			`"host":{"entry":"` + entry + `","resources":["` + entry + `"],"handlers":["initialize","completed","note"],"read_only":["note"],"turn":{"initialize":"initialize","before_model":null,"after_turn":"completed"}}}}}`,
		entry: `import {context,getConversationValue,prepareTransaction,completeRequest} from "sailry/sdk";
export async function initialize(args){
  return {tools:args.tools,instruction:"facts automatic="+args.automatic+" notice="+args.notice+" marker="+(args.continuation ? args.continuation.marker : "none")};
}
export async function note(){return {noted:true};}
export async function completed(args){
  const entry=await getConversationValue("loop");
  const seen=[...(entry.value?.seen ?? []),args];
  const operations=[{kind:"conversation_write",data:{key:"loop",value:{seen},expected_revision:entry.revision}}];
  if(!args.automatic) operations.push({kind:"continue",data:{session:context().session,after:args.turn,key:"loop",scope:"conversation",expected_revision:String(BigInt(entry.revision)+1n),marker:"verify",limit:20,message:{text:"Verify the fixture",references:[],attachments:[]}}});
  const result=await completeRequest(prepareTransaction(operations));
  if(result.Err) throw new Error(result.Err.message);
  return {};
}`,
	} {
		filename := filepath.Join(source, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filename, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	info, err := node.plugins.manager.Install(ctx, source, "", "turn-facts-fixture")
	if err != nil || len(info.Issues) != 0 || info.Extension == nil {
		t.Fatalf("fixture package is invalid: %+v, %v", info.Issues, err)
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := store.InstallPlugin(ctx, db, info, 0, true); err != nil {
			return err
		}
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Turn facts", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Start the fixture"},
	}})
	type tokens struct {
		Input  uint64 `json:"input"`
		Output uint64 `json:"output"`
	}
	var seen []struct {
		Turn         string         `json:"turn"`
		Status       string         `json:"status"`
		Mode         string         `json:"mode"`
		Automatic    bool           `json:"automatic"`
		Notice       bool           `json:"notice"`
		ToolCalls    int            `json:"tool_calls"`
		Tools        map[string]int `json:"tools"`
		Expired      int            `json:"expired"`
		Usage        tokens         `json:"usage"`
		SessionUsage tokens         `json:"session_usage"`
		Background   struct {
			Running int `json:"running"`
			Pending int `json:"pending"`
		} `json:"background"`
		Chain struct {
			Count int `json:"count"`
			Limit int `json:"limit"`
		} `json:"chain"`
		Continuation *struct {
			Marker *string `json:"marker"`
		} `json:"continuation"`
		Error *string `json:"error"`
	}
	var history protocol.ConversationHistory
	for len(seen) < 2 {
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			if history, err = store.ReadConversation(ctx, db, session.ID, nil, 100); err != nil {
				return err
			}
			entry, err := store.ReadPluginConversationValue(ctx, db, plugins.PackageReference(info), session.ID, "loop")
			if err != nil || !entry.Present {
				return err
			}
			var value struct {
				Seen json.RawMessage `json:"seen"`
			}
			if err = json.Unmarshal(entry.Value, &value); err != nil {
				return err
			}
			return json.Unmarshal(value.Seen, &seen)
		}); err != nil {
			t.Fatal(err)
		}
		for _, run := range history.Page.Runs {
			if run.Status == protocol.ConversationStatusFailed || run.Status == protocol.ConversationStatusInterrupted {
				t.Fatalf("fixture turn failed: %+v", run)
			}
		}
		if len(seen) < 2 {
			if _, err := subscription.Next(ctx); err != nil {
				t.Fatalf("after_turn was observed %d times: %v", len(seen), err)
			}
		}
	}
	runs := history.Page.Runs
	if len(runs) != 2 || runs[1].Continuation == nil || runs[1].Continuation.Kind == nil || *runs[1].Continuation.Kind != "verify" || runs[1].Continuation.Marker == nil || runs[1].Continuation.Marker.Label != "Verify work" {
		t.Fatalf("the continuation lost its marker: %+v", runs)
	}
	note := operations.Alias("turn-facts-fixture", "note")
	t.Run("user turn", func(t *testing.T) {
		fact := seen[0]
		if fact.Turn != runs[0].Turn.String() || fact.Status != "completed" || fact.Mode != "code" || fact.Automatic || fact.Notice || fact.Continuation != nil || fact.Error != nil || fact.Expired != 0 {
			t.Fatalf("origin facts = %+v", fact)
		}
		if fact.ToolCalls != 2 || fact.Tools[note] != 1 || fact.Tools["tool_search"] != 1 {
			t.Fatalf("tool facts = %d, %v", fact.ToolCalls, fact.Tools)
		}
		if fact.Usage != (tokens{370, 35}) || fact.SessionUsage != (tokens{370, 35}) {
			t.Fatalf("usage = %+v, session %+v", fact.Usage, fact.SessionUsage)
		}
		if fact.Chain.Count != 0 || fact.Chain.Limit != 8 || fact.Background.Running != 0 || fact.Background.Pending != 0 {
			t.Fatalf("chain = %+v, background = %+v", fact.Chain, fact.Background)
		}
	})
	t.Run("continuation", func(t *testing.T) {
		fact := seen[1]
		if fact.Turn != runs[1].Turn.String() || !fact.Automatic || fact.Notice || fact.Continuation == nil || fact.Continuation.Marker == nil || *fact.Continuation.Marker != "verify" {
			t.Fatalf("origin facts = %+v", fact)
		}
		if fact.ToolCalls != 0 || len(fact.Tools) != 0 {
			t.Fatalf("tool facts = %d, %v", fact.ToolCalls, fact.Tools)
		}
		if fact.Usage != (tokens{7, 3}) || fact.SessionUsage != (tokens{377, 38}) {
			t.Fatalf("usage = %+v, session %+v", fact.Usage, fact.SessionUsage)
		}
		if fact.Chain.Count != 1 || fact.Chain.Limit != 20 {
			t.Fatalf("chain = %+v", fact.Chain)
		}
	})
	t.Run("initialize", func(t *testing.T) {
		script.mu.Lock()
		prompts := append([]string(nil), script.prompts...)
		script.mu.Unlock()
		if len(prompts) != 4 {
			t.Fatalf("model requests = %d", len(prompts))
		}
		if !strings.Contains(prompts[0], "facts automatic=false notice=false marker=none") {
			t.Fatal("the user turn was initialized as automatic")
		}
		if !strings.Contains(prompts[3], "facts automatic=true notice=false marker=verify") {
			t.Fatal("the continuation was not initialized as automatic with its marker")
		}
	})
}
