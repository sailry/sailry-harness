package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type workspaceScript struct {
	fantasy.LanguageModel
	name, args string
	step       atomic.Int32
}

func (m *workspaceScript) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	step := m.step.Add(1)
	if step > 3 {
		return nil, errors.New("unexpected extra native tool model request")
	}
	return func(yield func(fantasy.StreamPart) bool) {
		switch step {
		case 1:
			args, _ := json.Marshal(map[string]any{"query": "select:" + m.name})
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: "discover", ToolCallName: "tool_search", ToolCallInput: string(args)}) {
				return
			}
		case 2:
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: "write", ToolCallName: m.name, ToolCallInput: m.args}) {
				return
			}
		case 3:
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Updated the file"}) {
				return
			}
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

func TestNodeHooksCapture(t *testing.T) {
	node := nodeFixture(t)
	ctx := t.Context()
	config := sessionConfigFixture(t, node)
	newProject := func(name string) protocol.ProjectID {
		draft := protocol.DefaultProjectsDraft()
		draft.Name, draft.Path, draft.Source = name, t.TempDir(), protocol.ProjectsSource{Kind: "local"}
		return commandFixture(t, node, protocol.Command{Kind: "create_project", Data: draft}).Data.(protocol.Project).ID
	}
	newSession := func(project *protocol.ProjectID) protocol.Session {
		return commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: project, Config: &config}}).Data.(protocol.Session)
	}
	first, second := newProject("First"), newProject("Second")
	sessions := []protocol.Session{newSession(&first), newSession(&second), newSession(nil)}
	queue := func(session protocol.Session) protocol.TurnID {
		return commandFixture(t, node, protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Keep the admitted hook settings"},
		}}).Data.(protocol.QueuedTurn).ID
	}
	checkCapture := func(session protocol.Session, turn protocol.TurnID, want protocol.HooksSettings) {
		t.Helper()
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			run, err := store.ReadAgentRun(ctx, db, turn)
			if err != nil {
				return err
			}
			settings, err := store.FrozenHooks(ctx, db, turn)
			if err != nil {
				return err
			}
			if run.Worktree != session.Worktree || !reflect.DeepEqual(settings, want) {
				return fmt.Errorf("turn %s: captured worktree %s and settings %+v, want %s and %+v", turn, run.Worktree, settings, session.Worktree, want)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
	defaults := protocol.DefaultHooksSettings()
	settings := defaults
	settings.Enabled = true
	settings.TurnEnd = &protocol.HooksScript{Name: "Legacy", Enabled: true, Command: "legacy hook", TimeoutMs: 1234}
	legacy, err := json.Marshal(settings)
	if err != nil {
		t.Fatal(err)
	}
	if err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, "INSERT INTO hook_settings(worktree,body) VALUES(?1,?2)", sessions[0].Worktree.String(), legacy)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	read := func() protocol.HooksState {
		return commandFixture(t, node, protocol.Command{Kind: "read_hooks"}).Data.(protocol.HooksState)
	}
	if state := read(); !reflect.DeepEqual(state.Settings, defaults) || state.Sandboxed != process.SandboxSupported() {
		t.Fatalf("fresh Node settings: %+v", state)
	}
	beforeSave := queue(sessions[0])
	save := func(settings protocol.HooksSettings) protocol.HooksSettings {
		command := protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Settings: settings, ExpectedRevision: settings.Revision}}
		return commandFixture(t, node, command).Data.(protocol.HooksState).Settings
	}
	settings.TurnEnd = &protocol.HooksScript{Name: "Node", Enabled: true, Command: "first Node hook", TimeoutMs: 1234}
	saved := save(settings)
	if saved.Revision != 1 || !reflect.DeepEqual(read().Settings, saved) {
		t.Fatalf("first Node save: %+v", saved)
	}
	var queued []protocol.TurnID
	for _, session := range sessions {
		queued = append(queued, queue(session))
	}
	stale := protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Settings: defaults}}
	if response := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), stale)); response.Fault == nil || response.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("stale Node save: %+v", response)
	}
	settings = saved
	settings.TurnEnd = &protocol.HooksScript{Name: "Node", Enabled: true, Command: "second Node hook", TimeoutMs: 1234}
	updated := save(settings)
	if updated.Revision != 2 || !reflect.DeepEqual(read().Settings, updated) {
		t.Fatalf("second Node save: %+v", updated)
	}
	checkCapture(sessions[0], beforeSave, defaults)
	for index, session := range sessions {
		checkCapture(session, queued[index], saved)
		checkCapture(session, queue(session), updated)
	}
}

func TestWorkspaceNativeTurn(t *testing.T) {
	catalog, err := operations.Bundled()
	if err != nil {
		t.Fatal(err)
	}
	script := &workspaceScript{}
	for _, tool := range catalog.Tools {
		if tool.Operation == protocol.ToolOperationWriteFile && tool.Available(true) {
			script.name = tool.Name
			break
		}
	}
	if script.name == "" {
		t.Fatal("bundled workspace write declaration is missing")
	}
	before := "before\n"
	expected := fmt.Sprintf("%x", blake3.Sum256([]byte(before)))
	args, _ := json.Marshal(map[string]any{"path": "note.txt", "text": "after\n", "expected_revision": expected})
	script.args = string(args)
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	type event struct {
		Version  int    `json:"schema_version"`
		Phase    string `json:"hook_event_name"`
		Node     string `json:"node_id"`
		Project  string `json:"project_id"`
		Worktree string `json:"worktree_id"`
		Session  string `json:"session_id"`
		Turn     string `json:"turn_id"`
		Cwd      string `json:"cwd"`
		Tool     string `json:"tool_name"`
		Call     string `json:"tool_use_id"`
		Status   string `json:"status"`
	}
	hookCalls := make(chan event, 5)
	processOptions := process.Options{
		After: func(time.Duration) <-chan time.Time { return make(chan time.Time) },
		Run: func(_ context.Context, launch process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
			if launch.Access != protocol.ProcessAccessWorkspace || launch.Root != root || launch.TimeoutMs != 1234 || launch.Background || launch.Pty {
				return protocol.ProcessCompletion{}, errors.New("hook escaped captured workspace policy")
			}
			var input event
			if err := json.Unmarshal([]byte(launch.Stdin), &input); err != nil {
				return protocol.ProcessCompletion{}, err
			}
			select {
			case hookCalls <- input:
			default:
				return protocol.ProcessCompletion{}, errors.New("unexpected extra hook execution")
			}
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
		},
	}
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{MCP: fixtureMCPOptions(), Environment: &environment, Process: processOptions, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil }})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	if err = os.WriteFile(filepath.Join(root, "note.txt"), []byte(before), 0600); err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{Name: "Fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance()}}).Data.(protocol.Project)
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Revision = 1
	provider.Enabled = true
	provider.Name = "Fixture"
	provider.Api = protocol.ConversationModelApiChatCompletions
	provider.Authentication = protocol.AuthenticationApiKey
	provider.Endpoint = "http://fixture.invalid/v1"
	provider.DefaultModel = "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID = "fixture"
	selected.Context = 32000
	selected.Output = 8000
	selected.Tools = true
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
	config.Provider = provider.ID
	config.Model = "fixture"
	config.Permission = protocol.PermissionAsk
	config.Mode = protocol.WorkModeCode
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	settings := protocol.DefaultHooksSettings()
	settings.Enabled = true
	settings.BeforeTool = &protocol.HooksScript{Name: "Fixture", Enabled: true, Command: "fixture hook", TimeoutMs: 1234}
	settings.AfterTool, settings.TurnEnd = settings.BeforeTool, settings.BeforeTool
	commandFixture(t, node, protocol.Command{Kind: "save_hooks", Data: protocol.CommandSaveHooksPayload{Settings: settings}})

	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Update note.txt"}}})
	var turn protocol.TurnID
	approved := false
	for {
		var history protocol.ConversationHistory
		err = node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State == protocol.ConversationApprovalStatePending {
				data, err := os.ReadFile(filepath.Join(root, "note.txt"))
				if err != nil || string(data) != before {
					t.Fatal("effect happened before approval", string(data), err)
				}
				if approval.Changes == nil {
					t.Fatal("native write approval omitted preview")
				}
				commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}})
				approved = true
			}
		}
		finished := false
		for _, run := range history.Page.Runs {
			turn = run.Turn
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				finished = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("native turn failed: %#v", run)
			}
		}
		if finished {
			break
		}
		if _, err = subscription.Next(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
	if !approved || script.step.Load() != 3 {
		t.Fatal("native approval/tool loop did not complete", approved, script.step.Load())
	}
	wantEvents := []struct{ phase, tool, call, status string }{
		{"before_tool", "tool_search", "discover", ""},
		{"after_tool", "tool_search", "discover", "succeeded"},
		{"before_tool", script.name, "write", ""},
		{"after_tool", script.name, "write", "succeeded"},
		{"turn_end", "", "", "completed"},
	}
	if len(hookCalls) != len(wantEvents) {
		t.Fatalf("received %d hook events, want %d", len(hookCalls), len(wantEvents))
	}
	for _, want := range wantEvents {
		got := <-hookCalls
		expected := event{Version: 1, Phase: want.phase, Node: node.ID().String(), Project: project.ID.String(), Worktree: session.Worktree.String(),
			Session: session.ID.String(), Turn: turn.String(), Cwd: root, Tool: want.tool, Call: want.call, Status: want.status}
		if got != expected {
			t.Fatalf("hook input %+v, want %+v", got, expected)
		}
	}
	if commands := node.processes.List(session.ID); len(commands) != 0 {
		t.Fatal("foreground hooks leaked into background command inventory", commands)
	}
	data, err := os.ReadFile(filepath.Join(root, "note.txt"))
	if err != nil || string(data) != "after\n" {
		t.Fatal("native write result missing", string(data), err)
	}
	output := commandFixture(t, node, protocol.Command{Kind: "list_file_checkpoints", Data: protocol.CommandListFileCheckpointsPayload{Session: session.ID, Turn: turn, Limit: 50}})
	checkpoints := output.Data.(protocol.ConversationCheckpointPage)
	if len(checkpoints.Files) != 1 || checkpoints.Files[0].Before == nil {
		t.Fatal("approved pre-effect checkpoint missing", checkpoints)
	}
	commandFixture(t, node, protocol.Command{Kind: "restore_file_checkpoint", Data: protocol.CommandRestoreFileCheckpointPayload{Session: session.ID, Checkpoint: checkpoints.Files[0].ID, Worktree: session.Worktree}})
	data, err = os.ReadFile(filepath.Join(root, "note.txt"))
	if err != nil || string(data) != before {
		t.Fatal("checkpoint restore did not restore original bytes", string(data), err)
	}
	// A controller that forgot the Undo repeats it; the restored file is no conflict.
	repeated := commandFixture(t, node, protocol.Command{Kind: "restore_file_checkpoint", Data: protocol.CommandRestoreFileCheckpointPayload{Session: session.ID, Checkpoint: checkpoints.Files[0].ID, Worktree: session.Worktree}})
	if restored, ok := repeated.Data.(protocol.ConversationCheckpointRestored); !ok || restored.Revision == nil || *restored.Revision != checkpoints.Files[0].Before.Revision {
		t.Fatal("repeated restore did not report the restored revision", repeated)
	}
	// That the change was undone is the Node's fact: the checkpoint list says
	// so to a controller that restarted or switched sessions.
	if checkpoints.Files[0].Restored {
		t.Fatal("the checkpoint was listed restored before its restore")
	}
	listed := commandFixture(t, node, protocol.Command{Kind: "list_file_checkpoints", Data: protocol.CommandListFileCheckpointsPayload{Session: session.ID, Turn: turn, Limit: 50}}).Data.(protocol.ConversationCheckpointPage)
	if len(listed.Files) != 1 || !listed.Files[0].Restored || listed.Files[0].ID != checkpoints.Files[0].ID {
		t.Fatalf("the restored checkpoint is listed %+v", listed.Files)
	}
}
