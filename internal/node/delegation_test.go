package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type delegationScript struct {
	fantasy.LanguageModel
	mu       sync.Mutex
	parent   protocol.SessionID
	root     string
	write    string
	children map[protocol.SessionID]*workspaceScript
	requests atomic.Int32
}

func (script *delegationScript) bind(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
	script.mu.Lock()
	defer script.mu.Unlock()
	if binding.Session == script.parent {
		return script, nil
	}
	if script.children[binding.Session] != nil {
		return nil, errors.New("child model was reconstructed")
	}
	args, err := json.Marshal(map[string]any{"path": binding.Session.String() + ".txt", "text": "child result\n", "expected_revision": nil})
	if err != nil {
		return nil, err
	}
	child := &workspaceScript{name: script.write, args: string(args)}
	script.children[binding.Session] = child
	return child, nil
}

func (script *delegationScript) Stream(_ context.Context, _ fantasy.Call) (fantasy.StreamResponse, error) {
	request := script.requests.Add(1)
	if request == 2 {
		script.mu.Lock()
		defer script.mu.Unlock()
		if len(script.children) != 2 {
			return nil, errors.New("parent resumed before its foreground wave was admitted")
		}
		for id, child := range script.children {
			bytes, err := os.ReadFile(filepath.Join(script.root, id.String()+".txt"))
			if child.step.Load() != 3 || err != nil || string(bytes) != "child result\n" {
				return nil, errors.New("parent resumed before a foreground child settled")
			}
		}
	}
	if request > 2 {
		return nil, errors.New("unexpected extra parent request")
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if request == 1 {
			for _, title := range []string{"First", "Second"} {
				args, _ := json.Marshal(map[string]any{"title": title, "task": "Write your result to your own file"})
				if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: title, ToolCallName: "spawn_agent", ToolCallInput: string(args)}) {
					return
				}
			}
		} else if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Integrated both children"}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

func TestChildApprovalPath(t *testing.T) {
	t.Parallel()
	catalog, err := operations.Bundled()
	if err != nil {
		t.Fatal(err)
	}
	script := &delegationScript{root: t.TempDir(), children: map[protocol.SessionID]*workspaceScript{}}
	for _, tool := range catalog.Tools {
		if tool.Operation == protocol.ToolOperationWriteFile && tool.Available(true) {
			script.write = tool.Name
			break
		}
	}
	if script.write == "" {
		t.Fatal("native write tool is unavailable")
	}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{Environment: &environment, Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }}, Model: script.bind})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: "Delegation", Path: script.root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance()}}).Data.(protocol.Project)
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision = protocol.NewProviderID(), 1
	provider.Enabled, provider.Name = true, "Fixture"
	provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey
	provider.Endpoint, provider.DefaultModel = "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	err = node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider.ID, "fixture"
	config.Permission, config.Mode = protocol.PermissionAsk, protocol.WorkModeFlagship
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	script.mu.Lock()
	script.parent = session.ID
	script.mu.Unlock()
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Delegate two independent file writes and integrate both"}}})
	approved := map[protocol.ApprovalID]bool{}
	for {
		var history protocol.ConversationHistory
		var childHistories []protocol.ConversationHistory
		err = node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
			if err != nil {
				return err
			}
			for _, child := range history.Page.Children {
				value, err := store.ReadConversation(t.Context(), db, child.Run.Session, nil, 100)
				if err != nil {
					return err
				}
				childHistories = append(childHistories, value)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
		pending := []protocol.ConversationApproval{}
		for _, child := range childHistories {
			for _, approval := range child.Page.Approvals {
				if approval.State == protocol.ConversationApprovalStatePending && !approved[approval.ID] {
					pending = append(pending, approval)
				}
			}
		}
		if len(pending) == 2 {
			wrong := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: pending[0].ID, Decision: protocol.ConversationDecision{Kind: "approve"}}}))
			if wrong.Fault == nil {
				t.Fatal("parent session resolved a child-owned approval")
			}
			for _, approval := range pending {
				commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: approval.Session, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}})
				approved[approval.ID] = true
			}
		}
		completed := false
		for _, run := range history.Page.Runs {
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				completed = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("delegated turn failed: %v; children=%#v", run.Error, history.Page.Children)
			}
		}
		if completed {
			if len(approved) != 2 || script.requests.Load() != 2 {
				t.Fatal("foreground children or owning approvals were skipped", len(approved), script.requests.Load())
			}
			break
		}
		if _, err = subscription.Next(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
}
