package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
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

// Over 1 s: each Node start installs and verifies the bundled packages.
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
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{MCP: fixtureMCPOptions(), Environment: &environment, Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }}, Model: script.bind})
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
	t.Run("package tool bound to delegation", reviewDelegation)
}

// reviewScript drives the bundled code-review package: it freezes a review
// scope, then delegates its groups through delegate_review, the package tool
// bound to agents.delegate.
type reviewScript struct {
	fantasy.LanguageModel
	mu       sync.Mutex
	parent   protocol.SessionID
	requests int
	offered  [][]string
	results  map[string]string
	children []string
}

func (script *reviewScript) bind(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
	script.mu.Lock()
	defer script.mu.Unlock()
	if binding.Session == script.parent {
		return script, nil
	}
	return &reviewChild{owner: script}, nil
}

// reviewChild answers the delegated task without tools and keeps the task the
// package prepared for it.
type reviewChild struct {
	fantasy.LanguageModel
	owner *reviewScript
}

func (child *reviewChild) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	var task strings.Builder
	for _, message := range call.Prompt {
		if message.Role != fantasy.MessageRoleUser {
			continue
		}
		for _, part := range message.Content {
			if text, found := fantasy.AsMessagePart[fantasy.TextPart](part); found {
				task.WriteString(text.Text)
			}
		}
	}
	child.owner.mu.Lock()
	child.owner.children = append(child.owner.children, task.String())
	child.owner.mu.Unlock()
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Evidence gathered"}) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

func (script *reviewScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	defer script.mu.Unlock()
	script.requests++
	var names []string
	for _, tool := range call.Tools {
		names = append(names, tool.GetName())
	}
	script.offered = append(script.offered, names)
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found {
				switch output := result.Output.(type) {
				case fantasy.ToolResultOutputContentText:
					script.results[result.ToolCallID] = output.Text
				case fantasy.ToolResultOutputContentError:
					script.results[result.ToolCallID] = "error: " + output.Error.Error()
				}
			}
		}
	}
	scope, delegate := operations.Alias("code-review", "review_scope"), operations.Alias("code-review", "delegate_review")
	var name, args string
	switch script.requests {
	case 1:
		name, args = "tool_search", `{"query":"select:`+scope+","+delegate+`"}`
	case 2:
		name, args = scope, `{"target":{"kind":"worktree","data":{"scope":"all"}}}`
	case 3:
		var frozen struct {
			ReviewID string `json:"review_id"`
			Groups   []struct {
				ID string `json:"id"`
			} `json:"groups"`
		}
		if err := json.Unmarshal([]byte(script.results["scope"]), &frozen); err != nil || frozen.ReviewID == "" || len(frozen.Groups) == 0 {
			return nil, fmt.Errorf("review scope result is unusable: %s", script.results["scope"])
		}
		groups := []string{}
		for _, group := range frozen.Groups {
			groups = append(groups, group.ID)
		}
		encoded, _ := json.Marshal(map[string]any{"review_id": frozen.ReviewID, "groups": groups, "record_receipt": false, "title": "Review evidence"})
		name, args = delegate, string(encoded)
	case 4:
	default:
		return nil, errors.New("unexpected extra review request")
	}
	id := map[int]string{1: "discover", 2: "scope", 3: "delegate"}[script.requests]
	return func(yield func(fantasy.StreamPart) bool) {
		if name != "" {
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: id, ToolCallName: name, ToolCallInput: args}) {
				return
			}
		} else if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Review delegated"}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

func reviewDelegation(t *testing.T) {
	script := &reviewScript{results: map[string]string{}}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{MCP: fixtureMCPOptions(), Environment: &environment, Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }}, Model: script.bind})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	repository, err := gogit.PlainInitWithOptions(root, &gogit.PlainInitOptions{InitOptions: gogit.InitOptions{DefaultBranch: plumbing.NewBranchReferenceName("main")}})
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(root, "source.go"), []byte("package source\n\nfunc Value() int { return 1 }\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	worktree, err := repository.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	if _, err = worktree.Add("source.go"); err != nil {
		t.Fatal(err)
	}
	if _, err = worktree.Commit("Initial review fixture", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(100, 0)}}); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(root, "source.go"), []byte("package source\n\nfunc Value() int { return 2 }\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: "Review", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance()}}).Data.(protocol.Project)
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	if err = node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Permission, config.Mode = provider.ID, "fixture", protocol.PermissionAsk, protocol.WorkModeCode
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	script.mu.Lock()
	script.parent = session.ID
	script.mu.Unlock()
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Review my change with a delegated reviewer"}}})
	var history protocol.ConversationHistory
	for completed := false; !completed; {
		if err = node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State == protocol.ConversationApprovalStatePending {
				commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: approval.Session, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}})
			}
		}
		for _, run := range history.Page.Runs {
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				completed = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				script.mu.Lock()
				t.Fatalf("review turn failed: %+v; results=%v", run.Error, script.results)
			}
		}
		if !completed {
			if _, err = subscription.Next(t.Context()); err != nil {
				t.Fatal(err)
			}
		}
	}
	script.mu.Lock()
	defer script.mu.Unlock()
	scope, delegate := script.results["scope"], script.results["delegate"]
	var frozen struct {
		ReviewID string `json:"review_id"`
	}
	if err = json.Unmarshal([]byte(scope), &frozen); err != nil || frozen.ReviewID == "" {
		t.Fatalf("review scope = %s", scope)
	}
	if script.requests != 4 || !strings.Contains(delegate, `"admitted":true`) || strings.Contains(delegate, "error") {
		t.Fatalf("delegate_review after %d requests = %s", script.requests, delegate)
	}
	// The child runs the task the package prepared, not the arguments the
	// model sent, and its origin is the call the model made.
	if len(script.children) != 1 || !strings.Contains(script.children[0], "Frozen review capture:") || !strings.Contains(script.children[0], frozen.ReviewID) || !strings.Contains(script.children[0], "Gather evidence only") {
		t.Fatalf("delegated tasks = %q", script.children)
	}
	if len(history.Page.Children) != 1 || history.Page.Children[0].Run.Status != protocol.ConversationStatusCompleted || history.Page.Children[0].Origin.Session != session.ID {
		t.Fatalf("children = %+v", history.Page.Children)
	}
	if !strings.Contains(delegate, history.Page.Children[0].Run.Session.String()) {
		t.Fatalf("delegate_review result %s names another child than %s", delegate, history.Page.Children[0].Run.Session)
	}
}
