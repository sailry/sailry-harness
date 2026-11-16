package node

import (
	"context"
	"database/sql"
	"path/filepath"
	"reflect"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSharedSessionQueuesUsersInCanonicalHistory(t *testing.T) {
	held := &connectionHeldModel{started: make(chan struct{}), release: make(chan struct{})}
	var once sync.Once
	finish := func() { once.Do(func() { close(held.release) }) }
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, MCP: fixtureMCPOptions(),
		Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return held, nil },
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		finish()
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	first, second := protocol.NodeID{96}, protocol.NodeID{97}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := store.PairLinkPeer(ctx, db, first, link.DirectionController); err != nil {
			return err
		}
		return store.PairLinkPeer(ctx, db, second, link.DirectionController)
	}); err != nil {
		t.Fatal(err)
	}
	call := func(caller protocol.NodeID, command protocol.Command) protocol.Output {
		t.Helper()
		accepted, err := node.Dispatch(ctx, caller, protocol.NewRequest(node.ID(), command))
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		select {
		case result := <-accepted.Completion:
			if result.Fault != nil {
				t.Fatalf("%s: %v", command.Kind, result.Fault)
			}
			return result.Output
		case <-ctx.Done():
			t.Fatal(context.Cause(ctx))
			return protocol.Output{}
		}
	}
	personal := map[protocol.NodeID]protocol.ConversationProvider{}
	for _, caller := range []protocol.NodeID{first, second} {
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Name = protocol.NewProviderID(), "Personal fixture"
		provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey
		provider.Endpoint, provider.Enabled, provider.DefaultModel = "https://shared-session.invalid/v1", true, "fixture"
		selected := protocol.DefaultConversationModel()
		selected.ID, selected.Context, selected.Output = "fixture", 200000, 8192
		provider.Models = []protocol.ConversationModel{selected}
		secret := protocol.NewSecret("personal-" + caller.String())
		personal[caller] = call(caller, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}}).Data.(protocol.ConversationProvider)
	}
	draft := protocol.DefaultProjectsDraft()
	draft.Name, draft.Path, draft.Visibility = "Shared", t.TempDir(), protocol.ProjectVisibilityShared
	draft.Source = protocol.ProjectsSource{Kind: "local"}
	project := call(first, protocol.Command{Kind: "create_project", Data: draft}).Data.(protocol.Project)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = personal[first].ID, "fixture"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	session := call(first, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	turns := []protocol.QueuedTurn{}
	for index, caller := range []protocol.NodeID{first, second} {
		current := call(caller, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: session.ID}}).Data.(protocol.Session)
		if index != 0 {
			config.Provider = personal[caller].ID
			current = call(caller, protocol.Command{Kind: "set_session_config", Data: protocol.CommandSetSessionConfigPayload{Session: current.ID, ExpectedRevision: current.Revision, Config: config}}).Data.(protocol.Session)
		}
		input := protocol.DefaultConversationInput()
		input.Text = []string{"First user message", "Second user message"}[index]
		turns = append(turns, call(caller, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: current.ID, ExpectedRevision: current.Revision, Message: input}}).Data.(protocol.QueuedTurn))
		if index == 0 {
			select {
			case <-held.started:
			case <-ctx.Done():
				t.Fatal(context.Cause(ctx))
			}
		}
	}
	finish()
	for {
		var runs [2]protocol.ConversationRun
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			for index, turn := range turns {
				var err error
				runs[index], err = store.ReadAgentRun(ctx, db, turn.ID)
				if err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		for _, run := range runs {
			if run.Error != nil {
				t.Fatal(run.Error)
			}
		}
		if runs[0].Status == protocol.ConversationStatusCompleted && runs[1].Status == protocol.ConversationStatusCompleted {
			if runs[0].Sequence >= runs[1].Sequence {
				t.Fatal("shared turns executed out of queue order")
			}
			break
		}
		select {
		case <-time.After(10 * time.Millisecond):
		case <-ctx.Done():
			t.Fatal(context.Cause(ctx))
		}
	}
	var original protocol.ConversationHistory
	for index, caller := range []protocol.NodeID{first, second} {
		history := call(caller, protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: session.ID, Limit: 50}}).Data.(protocol.ConversationHistory)
		if index == 0 {
			original = history
		} else if !reflect.DeepEqual(original, history) {
			t.Fatal("shared users received different canonical histories")
		}
	}
}
