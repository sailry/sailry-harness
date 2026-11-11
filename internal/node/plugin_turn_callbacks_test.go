package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// afterTurnModel answers every request with one sentence and counts them.
type afterTurnModel struct {
	fantasy.LanguageModel
	mu       sync.Mutex
	requests int
}

func (script *afterTurnModel) Stream(context.Context, fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	script.requests++
	step := script.requests
	script.mu.Unlock()
	if step > 2 {
		return nil, errors.New("unexpected extra model request")
	}
	return func(yield func(fantasy.StreamPart) bool) {
		_ = yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}) &&
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

// TestPluginAfterTurn runs a real turn beside a package whose after_turn
// handler continues the conversation once: the callback is called for the
// user's turn and again, marked automatic, for the turn it started.
func TestPluginAfterTurn(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	script := &afterTurnModel{}
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
		"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"after-turn-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read","storage.write","conversation.read","conversation.control"],"host":{"entry":"` + entry + `","resources":["` + entry + `"],"handlers":["initialize","completed"],"turn":{"initialize":"initialize","before_model":null,"after_turn":"completed"}}}}}`,
		entry: `import {context,getConversationValue,prepareTransaction,completeRequest} from "sailry/sdk";
export async function initialize(args){return {tools:args.tools};}
export async function completed(args){
  const entry=await getConversationValue("loop");
  const seen=[...(entry.value?.seen ?? []),{turn:args.turn,status:args.status,automatic:args.automatic}];
  const operations=[{kind:"conversation_write",data:{key:"loop",value:{seen},expected_revision:entry.revision}}];
  if(!args.automatic) operations.push({kind:"continue",data:{session:context().session,after:args.turn,key:"loop",scope:"conversation",expected_revision:String(BigInt(entry.revision)+1n),message:{text:"Continue the fixture",references:[],attachments:[]}}});
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
	info, err := node.plugins.manager.Install(ctx, source, "", "after-turn-fixture")
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
		Name: "After turn", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
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
	// The second callback has stored both observations once two turns ended.
	var seen []struct {
		Turn      string `json:"turn"`
		Status    string `json:"status"`
		Automatic bool   `json:"automatic"`
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
	if len(runs) != 2 || runs[1].Continuation == nil || runs[0].Status != protocol.ConversationStatusCompleted || runs[1].Status != protocol.ConversationStatusCompleted {
		t.Fatalf("the callback did not continue the conversation once: %+v", runs)
	}
	if seen[0].Turn != runs[0].Turn.String() || seen[0].Automatic || seen[0].Status != "completed" || seen[1].Turn != runs[1].Turn.String() || !seen[1].Automatic {
		t.Fatalf("callback inputs = %+v", seen)
	}
	script.mu.Lock()
	requests := script.requests
	script.mu.Unlock()
	if requests != 2 {
		t.Fatalf("model requests = %d", requests)
	}
}
