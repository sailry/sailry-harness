//go:build integration

package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// acceptanceWait bounds every wait of the acceptance tests that does not
// load a local model.
const acceptanceWait = 15 * time.Second

// acceptanceCall sends one command through Node.Dispatch and decodes its
// answer, or returns its fault.
func acceptanceCall[T any](t *testing.T, node *Node, command protocol.Command) (T, *protocol.Fault) {
	t.Helper()
	return acceptanceRequest[T](t, node, protocol.NewRequest(node.ID(), command), acceptanceWait)
}

func acceptanceRequest[T any](t *testing.T, node *Node, request protocol.Request, wait time.Duration) (T, *protocol.Fault) {
	t.Helper()
	var result T
	accepted, err := node.Dispatch(t.Context(), node.ID(), request)
	if err != nil {
		var failure *protocol.Fault
		if errors.As(err, &failure) {
			return result, failure
		}
		t.Fatalf("%s was not admitted: %v", request.Command.Kind, err)
	}
	select {
	case response, delivered := <-accepted.Completion:
		if !delivered {
			t.Fatalf("%s lost its completion", request.Command.Kind)
		}
		if response.Fault != nil {
			return result, response.Fault
		}
		// A durable replay returns decoded JSON, so every answer is read as JSON.
		body, err := json.Marshal(response.Output.Data)
		if err == nil && string(body) != "null" {
			err = json.Unmarshal(body, &result)
		}
		if err != nil {
			t.Fatalf("%s output %T: %v", request.Command.Kind, response.Output.Data, err)
		}
	case <-time.After(wait):
		t.Fatalf("%s did not complete within %s", request.Command.Kind, wait)
	}
	return result, nil
}

// acceptanceDo is acceptanceCall for a command that must succeed.
func acceptanceDo[T any](t *testing.T, node *Node, command protocol.Command) T {
	t.Helper()
	result, failure := acceptanceCall[T](t, node, command)
	if failure != nil {
		t.Fatalf("%s: %s %s", command.Kind, failure.Code, failure.Message)
	}
	return result
}

// acceptanceEventually polls a condition for at most acceptanceWait.
func acceptanceEventually(t *testing.T, label string, done func() bool) {
	t.Helper()
	deadline := time.Now().Add(acceptanceWait)
	for !done() {
		if time.Now().After(deadline) {
			t.Fatalf("%s: not within %s", label, acceptanceWait)
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func acceptanceFault(t *testing.T, label string, failure *protocol.Fault, code protocol.ErrorCode, message string) {
	t.Helper()
	if failure == nil || failure.Code != code || failure.Message != message {
		t.Fatalf("%s: fault %+v, want %s %q", label, failure, code, message)
	}
}

// acceptanceStep is one scripted model response: a tool call, or the final
// answer when tool is empty.
type acceptanceStep struct {
	id, tool  string
	arguments func() string
}

// acceptanceScript is a language model that plays fixed steps and records the
// tools each request offered.
type acceptanceScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	steps   []acceptanceStep
	next    int
	offered [][]string
	prompt  fantasy.Prompt
}

// answered returns the tool result text the model was given for a call. A
// step's arguments read it while the script holds its lock.
func (script *acceptanceScript) answered(call string) string {
	for _, message := range script.prompt {
		for _, part := range message.Content {
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found && result.ToolCallID == call {
				if text, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output); found {
					return text.Text
				}
			}
		}
	}
	return ""
}

func (script *acceptanceScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	defer script.mu.Unlock()
	script.prompt = call.Prompt
	if script.next >= len(script.steps) {
		return nil, errors.New("unexpected extra acceptance model request")
	}
	step := script.steps[script.next]
	script.next++
	names := make([]string, 0, len(call.Tools))
	for _, tool := range call.Tools {
		names = append(names, tool.GetName())
	}
	script.offered = append(script.offered, names)
	part := fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}
	if step.tool != "" {
		part = fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: step.id, ToolCallName: step.tool, ToolCallInput: step.arguments()}
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(part) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

func (script *acceptanceScript) requests() [][]string {
	script.mu.Lock()
	defer script.mu.Unlock()
	return slices.Clone(script.offered)
}

func acceptanceArguments(value any) func() string {
	return func() string {
		body, _ := json.Marshal(value)
		return string(body)
	}
}

// acceptanceNode starts a real Node whose turns run the scripted model.
func acceptanceNode(t *testing.T, profile string, script *acceptanceScript, options Options) *Node {
	t.Helper()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	options.Environment = &environment
	if options.MCP == nil {
		options.MCP = fixtureMCPOptions()
	}
	if script != nil {
		options.Model = func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil }
	}
	node, err := StartWithOptions(t.Context(), profile, options)
	if err != nil {
		t.Fatal(err)
	}
	return node
}

// acceptanceSession registers a fixture provider and opens a conversation in
// the project.
func acceptanceSession(t *testing.T, node *Node, project protocol.ProjectID, permission protocol.Permission) protocol.Session {
	t.Helper()
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 200000, 8000, true
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
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, permission
	return acceptanceDo[protocol.Session](t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project, Config: &config}})
}

// acceptanceTurn submits one message and runs the turn to completion within
// wait. Every pending approval is handed to approve before it is approved. It
// returns the packages frozen for the turn and the tool responses by call.
func acceptanceTurn(t *testing.T, node *Node, session protocol.Session, text string, wait time.Duration, approve func(protocol.ConversationApproval)) ([]protocol.PluginReference, map[string]*canonical.ToolResponse) {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), wait)
	defer cancel()
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	queued := acceptanceDo[protocol.QueuedTurn](t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: text},
	}})
	seen := map[protocol.ApprovalID]bool{}
	for completed := false; !completed; {
		var history protocol.ConversationHistory
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(ctx, db, session.ID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State != protocol.ConversationApprovalStatePending || seen[approval.ID] {
				continue
			}
			seen[approval.ID] = true
			if approve == nil {
				t.Fatalf("the turn asked for an unexpected approval: %+v", approval)
			}
			approve(approval)
			acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{
				Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"},
			}})
		}
		for _, run := range history.Page.Runs {
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				completed = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("turn failed: %+v", run)
			}
		}
		if !completed {
			if _, err := subscription.Next(ctx); err != nil {
				t.Fatalf("turn did not complete within %s: %v", wait, err)
			}
		}
	}
	var state store.AgentState
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		var err error
		state, err = store.LoadAgentState(t.Context(), db, session.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	results := map[string]*canonical.ToolResponse{}
	for _, record := range state.Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if response, ok := block.Data.(*canonical.ToolResponse); ok {
					results[response.ID] = response
				}
			}
		}
	}
	return queued.Plugins, results
}

// acceptanceResult decodes a successful tool response.
func acceptanceResult(t *testing.T, results map[string]*canonical.ToolResponse, call, tool string) map[string]any {
	t.Helper()
	response := results[call]
	if response == nil {
		t.Fatalf("%s returned no response", call)
	}
	if response.Name != tool || response.Status != canonical.ToolSuccess {
		t.Fatalf("%s (%s) status %v: %s", call, response.Name, response.Status, response.Result.Value)
	}
	var value map[string]any
	if err := json.Unmarshal(response.Result.Value, &value); err != nil {
		t.Fatalf("%s result %s: %v", call, response.Result.Value, err)
	}
	return value
}

var codeFixture = map[string]string{
	"cmd/server.go": `package main

// Config holds the listen address.
type Config struct {
	Address string
}

func main() {
	Serve(Config{Address: ":8080"})
}

// Serve starts the listener.
func Serve(config Config) {
	helper(config.Address)
}

func helper(address string) {
	_ = address
}
`,
	"app/service.py": `class Service:
    def run(self):
        return compute(2)


def compute(value):
    return value * 2
`,
	"src/page.ts": `export function format(value: string): string {
  return value.trim();
}

export class Page {
  render(): string {
    return format(" page ");
  }
}
`,
	"lib/index.rs": `pub struct Index {
    pub entries: usize,
}

pub fn build() -> Index {
    open()
}

fn open() -> Index {
    Index { entries: 0 }
}
`,
	"settings.json": `{"name": "fixture", "port": 8080}
`,
	"README.md": "# Fixture\n\nA small project in several languages.\n",
}

// codeProject writes the fixture into a new Git repository with one commit
// and returns its root and commit.
func codeProject(t *testing.T, files map[string]string) (string, string) {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	repository, err := gogit.PlainInitWithOptions(root, &gogit.PlainInitOptions{InitOptions: gogit.InitOptions{DefaultBranch: plumbing.NewBranchReferenceName("main")}})
	if err != nil {
		t.Fatal(err)
	}
	tree, err := repository.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	for name, text := range files {
		path := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(text), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := tree.Add(name); err != nil {
			t.Fatal(err)
		}
	}
	commit, err := tree.Commit("Add fixture", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(100, 0)}})
	if err != nil {
		t.Fatal(err)
	}
	return root, commit.String()
}

// codeMain returns the main worktree of a project.
func codeMain(t *testing.T, node *Node, project protocol.ProjectID) protocol.WorktreeID {
	t.Helper()
	for _, tree := range snapshotFixture(t, node).Worktrees {
		if tree.Project != nil && *tree.Project == project && tree.Main {
			return tree.ID
		}
	}
	t.Fatal("project has no main worktree")
	return protocol.WorktreeID{}
}
