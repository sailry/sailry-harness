package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// scheduleScript answers with text, except that it first makes the one tool
// call a test has queued, discovering the tool when the request defers it.
type scheduleScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	pending *[2]string
	result  string
}

func (script *scheduleScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	defer script.mu.Unlock()
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found && result.ToolCallID == "task" {
				if text, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output); found {
					script.result = text.Text
				}
			}
		}
	}
	part := fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}
	if script.pending != nil {
		name, arguments := script.pending[0], script.pending[1]
		if slices.ContainsFunc(call.Tools, func(tool fantasy.Tool) bool { return tool.GetName() == name }) {
			part, script.pending = fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: "task", ToolCallName: name, ToolCallInput: arguments}, nil
		} else {
			part = fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: "discover", ToolCallName: "tool_search", ToolCallInput: `{"query":"select:` + name + `"}`}
		}
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(part) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

// TestScheduledTaskStart fires the bundled scheduled-tasks package's own
// callbacks: a schedule starts its session in the worktree the task stored,
// and a task created in a team conversation runs with that team.
func TestScheduledTaskStart(t *testing.T) {
	script := &scheduleScript{}
	// Task tools compare a start time with the script engine's wall clock, so
	// the dispatch clock starts at the same instant.
	clock := clockwork.NewFakeClockAt(time.Now())
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		DispatchClock: clock, Environment: &environment,
		Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("a scheduled task must not launch a process")
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	ctx := t.Context()
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 32000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	var scope protocol.PluginContext
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body); err != nil {
			return err
		}
		info, err := store.ReadPlugin(ctx, db, "scheduled-tasks")
		if err != nil {
			return err
		}
		if info == nil || !info.Summary.Enabled {
			return errors.New("the bundled scheduled-tasks package is not installed and enabled")
		}
		scope = protocol.DefaultPluginContext()
		scope.Package = plugins.PackageReference(*info)
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Scheduled", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionAsk
	// The project's own worktree is what a project conversation stores.
	worktree := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session).Worktree

	// call runs one of the package's management handlers as its page does.
	call := func(t *testing.T, handler string, input any) json.RawMessage {
		t.Helper()
		body, err := json.Marshal(input)
		if err != nil {
			t.Fatal(err)
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "call_plugin", Data: protocol.CommandCallPluginPayload{Handler: handler, Input: body}})
		request.Plugin = &scope
		response := dispatchFixture(t, node, node.ID(), request)
		if response.Fault != nil {
			t.Fatalf("%s: %v", handler, response.Fault)
		}
		raw, _ := json.Marshal(response.Output.Data)
		var result struct {
			Ok  json.RawMessage `json:"Ok"`
			Err *protocol.Fault `json:"Err"`
		}
		if err := json.Unmarshal(raw, &result); err != nil || result.Err != nil || result.Ok == nil {
			t.Fatalf("%s: %s %v", handler, raw, err)
		}
		return result.Ok
	}
	// fired advances the dispatch clock until the task's schedule has run, and
	// returns the session its callback started.
	fired := func(t *testing.T, task string) protocol.Session {
		t.Helper()
		var session protocol.Session
		deadline := time.Now().Add(60 * time.Second)
		for {
			// The dispatcher may be between timers; each step lets a new one expire.
			clock.Advance(time.Minute)
			var jobs []protocol.DispatchJob
			if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				rows, err := db.QueryContext(ctx, "SELECT body FROM dispatch_jobs WHERE package='scheduled-tasks' AND json_extract(body,'$.handler')=?1", task)
				if err != nil {
					return err
				}
				defer rows.Close()
				for rows.Next() {
					var body []byte
					var job protocol.DispatchJob
					if err := rows.Scan(&body); err != nil {
						return err
					}
					if err := json.Unmarshal(body, &job); err != nil {
						return err
					}
					jobs = append(jobs, job)
				}
				if err := rows.Err(); err != nil || len(jobs) != 1 || jobs[0].Status != protocol.DispatchStatusCompleted {
					return err
				}
				var id string
				if err := db.QueryRowContext(ctx, "SELECT session FROM session_associations WHERE namespace='scheduled-tasks' AND kind='task' AND identity=?1", task).Scan(&id); err != nil {
					return fmt.Errorf("the completed fire left no associated session: %w", err)
				}
				identity, err := protocol.ParseSessionID(id)
				if err != nil {
					return err
				}
				session, err = store.ReadSession(ctx, db, identity)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if len(jobs) > 1 {
				t.Fatalf("a once schedule fired %d times", len(jobs))
			}
			if len(jobs) == 1 {
				switch jobs[0].Status {
				case protocol.DispatchStatusCompleted:
					return session
				case protocol.DispatchStatusFailed, protocol.DispatchStatusCancelled, protocol.DispatchStatusUnknown:
					t.Fatalf("the fire ended %s: %v", jobs[0].Status, jobs[0].Error)
				}
			}
			if time.Now().After(deadline) {
				t.Fatalf("the schedule did not complete: %+v", jobs)
			}
		}
	}

	t.Run("a schedule starts its session in the stored worktree", func(t *testing.T) {
		id := protocol.NewRequestID().String()
		saved := call(t, "save", map[string]any{
			"id": id, "revision": "0", "name": "Nightly check", "prompt": "Check the build", "project": project.ID, "worktree": worktree,
			"config": config, "execution": "model", "enabled": true, "queue": "default",
			"timing": map[string]any{"kind": "once", "data": map[string]any{"at_ms": clock.Now().Add(time.Hour).UnixMilli()}},
		})
		var task struct {
			Worktree *protocol.WorktreeID `json:"worktree"`
		}
		if err := json.Unmarshal(saved, &task); err != nil || task.Worktree == nil || *task.Worktree != worktree {
			t.Fatalf("the task did not store its worktree: %s %v", saved, err)
		}
		clock.Advance(time.Hour)
		session := fired(t, id)
		if session.Worktree != worktree || session.Project == nil || *session.Project != project.ID {
			t.Fatalf("the fire started its session outside the stored worktree: %+v", session)
		}
		if session.Association == nil || *session.Association != (protocol.ConversationAssociation{Namespace: "scheduled-tasks", Kind: "task", ID: id}) {
			t.Fatalf("the started session lost its task association: %+v", session.Association)
		}
		if session.Roles.Execution != protocol.ConversationExecutionModel || session.Roles.Team != nil {
			t.Fatalf("a model task ran with a roster: %+v", session.Roles)
		}
	})

	t.Run("a task created in a team conversation runs with that team", func(t *testing.T) {
		role := protocol.DefaultRoleProfile()
		role.ID, role.Key, role.Name = protocol.NewRoleID(), "reviewer", "Reviewer"
		role = commandFixture(t, node, protocol.Command{Kind: "put_role", Data: protocol.CommandPutRolePayload{Role: role}}).Data.(protocol.RoleProfile)
		team := protocol.DefaultTeamProfile()
		team.ID, team.Key, team.Name, team.Members = protocol.NewTeamID(), "review", "Review", []protocol.RoleID{role.ID}
		team = commandFixture(t, node, protocol.Command{Kind: "put_team", Data: protocol.CommandPutTeamPayload{Team: team}}).Data.(protocol.TeamProfile)

		arguments, err := json.Marshal(map[string]any{"action": "create", "name": "Weekly review", "prompt": "Review the open changes",
			"timing": map[string]any{"kind": "once", "data": map[string]any{"at_ms": clock.Now().Add(2 * time.Hour).UnixMilli()}}})
		if err != nil {
			t.Fatal(err)
		}
		script.mu.Lock()
		script.pending = &[2]string{operations.Alias("scheduled-tasks", "scheduled_tasks"), string(arguments)}
		script.mu.Unlock()
		subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
		if err != nil {
			t.Fatal(err)
		}
		defer subscription.Close()
		start := protocol.DefaultConversationStart()
		start.Project, start.Config, start.Title = &project.ID, &config, "Plan the review"
		start.Message = protocol.ConversationInput{Text: "Schedule a weekly review"}
		start.Execution, start.Team = protocol.ConversationExecutionTeam, &protocol.TeamSelection{ID: team.ID}
		conversation := commandFixture(t, node, protocol.Command{Kind: "start_session", Data: start}).Data.(protocol.QueuedTurn).Session
		for completed := false; !completed; {
			var history protocol.ConversationHistory
			if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(ctx, db, conversation, nil, 100)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, approval := range history.Page.Approvals {
				if approval.State == protocol.ConversationApprovalStatePending {
					commandFixture(t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: conversation, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}})
				}
			}
			for _, run := range history.Page.Runs {
				switch run.Status {
				case protocol.ConversationStatusCompleted:
					completed = true
				case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
					t.Fatalf("the conversation turn failed: %+v", run)
				}
			}
			if !completed {
				if _, err := subscription.Next(ctx); err != nil {
					t.Fatal(err)
				}
			}
		}
		script.mu.Lock()
		result := script.result
		script.mu.Unlock()
		var task struct {
			ID        string               `json:"id"`
			Project   *protocol.ProjectID  `json:"project"`
			Worktree  *protocol.WorktreeID `json:"worktree"`
			Execution string               `json:"execution"`
			Team      *struct {
				ID protocol.TeamID `json:"id"`
			} `json:"team"`
		}
		if err := json.Unmarshal([]byte(result), &task); err != nil || task.ID == "" {
			t.Fatalf("the tool did not create a task: %q %v", result, err)
		}
		if task.Execution != "team" || task.Team == nil || task.Team.ID != team.ID {
			t.Fatalf("the task lost the conversation's team: %s", result)
		}
		if task.Project == nil || *task.Project != project.ID || task.Worktree == nil || *task.Worktree != worktree {
			t.Fatalf("the task lost the conversation's project or worktree: %s", result)
		}
		clock.Advance(2 * time.Hour)
		session := fired(t, task.ID)
		if session.Worktree != worktree {
			t.Fatalf("the fire started its session outside the stored worktree: %+v", session)
		}
		if session.Roles.Execution != protocol.ConversationExecutionTeam || session.Roles.Team == nil || session.Roles.Team.ID != team.ID || session.Roles.Team.Revision != team.Revision {
			t.Fatalf("the fire ran without the task's team: %+v", session.Roles)
		}
		if len(session.Roles.Profiles) != 1 || session.Roles.Profiles[0].ID != role.ID {
			t.Fatalf("the fire ran without the team's members: %+v", session.Roles.Profiles)
		}
	})
}
