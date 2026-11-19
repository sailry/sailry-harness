package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestUserStopEndsCommandWork stops a turn that started a test command and a
// declared service. The stop ends the test command, leaves the service running,
// and the service's later completion waits for the user's next message instead
// of starting a turn.
func TestUserStopEndsCommandWork(t *testing.T) {
	script := &agentToolScript{}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	blocking, testsStopped, serviceStopped, serviceExit := make(chan struct{}), make(chan struct{}), make(chan struct{}), make(chan struct{})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(ctx context.Context, launch process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
			cancelled := protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}
			switch launch.Command {
			case "go test ./...":
				<-ctx.Done()
				close(testsStopped)
				return cancelled, nil
			case "serve":
				select {
				case <-ctx.Done():
					close(serviceStopped)
					return cancelled, nil
				case <-serviceExit:
					return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
				}
			case "block":
				close(blocking)
				<-ctx.Done()
				return cancelled, nil
			}
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
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
		Name: "Stopped commands", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, protocol.PermissionFull
	session := commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)

	script.reset(func(step int, _ map[string]string) [][3]string {
		switch step {
		case 1:
			return [][3]string{
				{"tests", "run_command", `{"command":"go test ./...","background":true,"follow_up":true}`},
				{"server", "run_command", `{"command":"serve","background":true,"follow_up":true,"service":true}`},
			}
		case 2:
			return [][3]string{{"block", "run_command", `{"command":"block"}`}}
		}
		return nil
	})
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Run the tests and the server"},
	}})
	wait := func(name string, done <-chan struct{}) {
		t.Helper()
		select {
		case <-done:
		case <-time.After(10 * time.Second):
			t.Fatalf("timed out waiting for %s", name)
		}
	}
	read := func() ([]protocol.ConversationRun, []protocol.ConversationBackgroundTask, protocol.ConversationQueue) {
		t.Helper()
		var runs []protocol.ConversationRun
		var tasks []protocol.ConversationBackgroundTask
		var queue protocol.ConversationQueue
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			history, err := store.ReadConversation(t.Context(), db, session.ID, nil, 100)
			if err != nil {
				return err
			}
			runs = history.Page.Runs
			if tasks, err = store.ReadBackgroundTasks(t.Context(), db, session.ID); err != nil {
				return err
			}
			queue, err = store.ReadQueue(t.Context(), db, session.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return runs, tasks, queue
	}
	until := func(name string, done func([]protocol.ConversationRun, []protocol.ConversationBackgroundTask) bool) ([]protocol.ConversationRun, []protocol.ConversationBackgroundTask) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for {
			runs, tasks, _ := read()
			if done(runs, tasks) {
				return runs, tasks
			}
			if time.Now().After(deadline) {
				t.Fatalf("timed out waiting for %s: runs %+v, background %+v", name, runs, tasks)
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
	task := func(tasks []protocol.ConversationBackgroundTask, title string) protocol.ConversationBackgroundTask {
		t.Helper()
		index := slices.IndexFunc(tasks, func(task protocol.ConversationBackgroundTask) bool { return task.Title == title })
		if index < 0 {
			t.Fatalf("no background record for %s in %+v", title, tasks)
		}
		return tasks[index]
	}

	wait("the blocking foreground command", blocking)
	runs, _, _ := read()
	commandFixture(t, node, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: runs[0].Turn}})
	wait("the stop of the test command", testsStopped)
	runs, tasks := until("the stopped turn and test result", func(runs []protocol.ConversationRun, tasks []protocol.ConversationBackgroundTask) bool {
		return len(runs) == 1 && runs[0].Status == protocol.ConversationStatusCancelled && slices.ContainsFunc(tasks, func(task protocol.ConversationBackgroundTask) bool {
			return task.Title == "go test ./..." && task.State == protocol.ConversationBackgroundStatePending
		})
	})
	if tests := task(tasks, "go test ./..."); tests.Outcome == nil || *tests.Outcome != protocol.ConversationBackgroundOutcomeStopped || !tests.Suppressed {
		t.Fatalf("the stopped test command is recorded as %+v", tests)
	}
	server := task(tasks, "serve")
	if server.State != protocol.ConversationBackgroundStateRunning || !server.Suppressed || server.Hold == nil || *server.Hold != protocol.ConversationBackgroundHoldStopped {
		t.Fatalf("the service after the stop is recorded as %+v", server)
	}
	select {
	case <-serviceStopped:
		t.Fatal("the stop ended the declared service")
	default:
	}
	if active := slices.ContainsFunc(node.processes.List(session.ID), func(info protocol.ProcessInfo) bool {
		return info.Command == "serve" && info.Status.Kind == "running"
	}); !active {
		t.Fatal("the declared service is not running after the stop")
	}

	// Resuming the stopped queue does not let the service's completion start a turn.
	_, _, queue := read()
	commandFixture(t, node, protocol.Command{Kind: "set_queue_paused", Data: protocol.CommandSetQueuePausedPayload{Session: session.ID, ExpectedRevision: queue.Revision, Paused: false}})
	close(serviceExit)
	_, tasks = until("the service result", func(_ []protocol.ConversationRun, tasks []protocol.ConversationBackgroundTask) bool {
		return slices.ContainsFunc(tasks, func(task protocol.ConversationBackgroundTask) bool {
			return task.Title == "serve" && task.State == protocol.ConversationBackgroundStatePending
		})
	})
	if server = task(tasks, "serve"); server.Outcome == nil || *server.Outcome != protocol.ConversationBackgroundOutcomeCompleted || !server.Suppressed || server.Delivery != nil {
		t.Fatalf("the service completion after the stop is recorded as %+v", server)
	}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		turn, _, err := store.ScheduleBackground(t.Context(), db, node.ID(), session.ID, time.Now())
		if err == nil && turn != nil {
			t.Errorf("a completion after the stop admitted notice turn %+v", turn)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if runs, _, _ = read(); len(runs) != 1 {
		t.Fatalf("a completion after the stop started a turn: %+v", runs)
	}

	// The user's next message carries both outcomes.
	script.reset(func(int, map[string]string) [][3]string { return nil })
	current := commandFixture(t, node, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: session.ID}}).Data.(protocol.Session)
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: current.Revision, Message: protocol.ConversationInput{Text: "What happened?"},
	}})
	runs, tasks = until("the next user turn", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return len(runs) == 2 && slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool { return run.Status == protocol.ConversationStatusCompleted })
	})
	next := runs[slices.IndexFunc(runs, func(run protocol.ConversationRun) bool { return run.Status == protocol.ConversationStatusCompleted })]
	if next.Notice {
		t.Fatalf("the next turn is automatic: %+v", next)
	}
	for _, title := range []string{"go test ./...", "serve"} {
		if delivered := task(tasks, title); delivered.State != protocol.ConversationBackgroundStateDelivered || delivered.Delivery == nil || *delivered.Delivery != next.Turn {
			t.Errorf("%s is not delivered to the user's next turn: %+v", title, delivered)
		}
	}
}
