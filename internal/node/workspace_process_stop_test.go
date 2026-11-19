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

type commandRun func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error)

// stopFixture is a Node with a scripted model and scripted commands, and one
// full-permission session in a local project.
type stopFixture struct {
	t       *testing.T
	node    *Node
	script  *agentToolScript
	session protocol.Session
}

func newStopFixture(t *testing.T, run commandRun) stopFixture {
	t.Helper()
	script := &agentToolScript{}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: run},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { node.Stop(context.Background()) })
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
	return stopFixture{t: t, node: node, script: script, session: session}
}

func (f stopFixture) wait(name string, done <-chan struct{}) {
	f.t.Helper()
	select {
	case <-done:
	case <-time.After(10 * time.Second):
		f.t.Fatalf("timed out waiting for %s", name)
	}
}

func (f stopFixture) read() ([]protocol.ConversationRun, []protocol.ConversationBackgroundTask, protocol.ConversationQueue) {
	f.t.Helper()
	var runs []protocol.ConversationRun
	var tasks []protocol.ConversationBackgroundTask
	var queue protocol.ConversationQueue
	if err := f.node.profile.Store.Node.Read(f.t.Context(), func(db *sql.Conn) error {
		history, err := store.ReadConversation(f.t.Context(), db, f.session.ID, nil, 100)
		if err != nil {
			return err
		}
		runs = history.Page.Runs
		if tasks, err = store.ReadBackgroundTasks(f.t.Context(), db, f.session.ID); err != nil {
			return err
		}
		queue, err = store.ReadQueue(f.t.Context(), db, f.session.ID)
		return err
	}); err != nil {
		f.t.Fatal(err)
	}
	return runs, tasks, queue
}

func (f stopFixture) until(name string, done func([]protocol.ConversationRun, []protocol.ConversationBackgroundTask) bool) ([]protocol.ConversationRun, []protocol.ConversationBackgroundTask) {
	f.t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for {
		runs, tasks, _ := f.read()
		if done(runs, tasks) {
			return runs, tasks
		}
		if time.Now().After(deadline) {
			f.t.Fatalf("timed out waiting for %s: runs %+v, background %+v", name, runs, tasks)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func (f stopFixture) task(tasks []protocol.ConversationBackgroundTask, title string) protocol.ConversationBackgroundTask {
	f.t.Helper()
	index := slices.IndexFunc(tasks, func(task protocol.ConversationBackgroundTask) bool { return task.Title == title })
	if index < 0 {
		f.t.Fatalf("no background record for %s in %+v", title, tasks)
	}
	return tasks[index]
}

func (f stopFixture) submit(text string) {
	f.t.Helper()
	current := commandFixture(f.t, f.node, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: f.session.ID}}).Data.(protocol.Session)
	commandFixture(f.t, f.node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: f.session.ID, ExpectedRevision: current.Revision, Message: protocol.ConversationInput{Text: text},
	}})
}

// TestUserStopEndsCommandWork stops a turn that started a test command and
// declared services. The stop ends the test command and leaves the services
// running. A service's later completion waits for the user's next message
// instead of starting a turn, and that message releases the hold on a service
// still running.
func TestUserStopEndsCommandWork(t *testing.T) {
	blocking, testsStopped, serviceStopped, serviceExit := make(chan struct{}), make(chan struct{}), make(chan struct{}), make(chan struct{})
	f := newStopFixture(t, func(ctx context.Context, launch process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
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
		case "watch", "block":
			if launch.Command == "block" {
				close(blocking)
			}
			<-ctx.Done()
			return cancelled, nil
		}
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
	})
	f.script.reset(func(step int, _ map[string]string) [][3]string {
		switch step {
		case 1:
			return [][3]string{
				{"tests", "run_command", `{"command":"go test ./...","background":true,"follow_up":true}`},
				{"server", "run_command", `{"command":"serve","background":true,"follow_up":true,"service":true}`},
				{"watcher", "run_command", `{"command":"watch","background":true,"follow_up":true,"service":true}`},
			}
		case 2:
			return [][3]string{{"block", "run_command", `{"command":"block"}`}}
		}
		return nil
	})
	f.submit("Run the tests and the servers")

	f.wait("the blocking foreground command", blocking)
	runs, _, _ := f.read()
	commandFixture(t, f.node, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: runs[0].Turn}})
	f.wait("the stop of the test command", testsStopped)
	runs, tasks := f.until("the stopped turn and test result", func(runs []protocol.ConversationRun, tasks []protocol.ConversationBackgroundTask) bool {
		return len(runs) == 1 && runs[0].Status == protocol.ConversationStatusCancelled && slices.ContainsFunc(tasks, func(task protocol.ConversationBackgroundTask) bool {
			return task.Title == "go test ./..." && task.State == protocol.ConversationBackgroundStatePending
		})
	})
	if tests := f.task(tasks, "go test ./..."); tests.Outcome == nil || *tests.Outcome != protocol.ConversationBackgroundOutcomeStopped || !tests.Suppressed {
		t.Fatalf("the stopped test command is recorded as %+v", tests)
	}
	for _, title := range []string{"serve", "watch"} {
		if server := f.task(tasks, title); server.State != protocol.ConversationBackgroundStateRunning || !server.Suppressed || server.Hold == nil || *server.Hold != protocol.ConversationBackgroundHoldStopped {
			t.Fatalf("the service %s after the stop is recorded as %+v", title, server)
		}
	}
	select {
	case <-serviceStopped:
		t.Fatal("the stop ended the declared service")
	default:
	}
	if active := slices.ContainsFunc(f.node.processes.List(f.session.ID), func(info protocol.ProcessInfo) bool {
		return info.Command == "serve" && info.Status.Kind == "running"
	}); !active {
		t.Fatal("the declared service is not running after the stop")
	}

	// Resuming the stopped queue does not let the service's completion start a turn.
	_, _, queue := f.read()
	commandFixture(t, f.node, protocol.Command{Kind: "set_queue_paused", Data: protocol.CommandSetQueuePausedPayload{Session: f.session.ID, ExpectedRevision: queue.Revision, Paused: false}})
	close(serviceExit)
	_, tasks = f.until("the service result", func(_ []protocol.ConversationRun, tasks []protocol.ConversationBackgroundTask) bool {
		return slices.ContainsFunc(tasks, func(task protocol.ConversationBackgroundTask) bool {
			return task.Title == "serve" && task.State == protocol.ConversationBackgroundStatePending
		})
	})
	if server := f.task(tasks, "serve"); server.Outcome == nil || *server.Outcome != protocol.ConversationBackgroundOutcomeCompleted || !server.Suppressed || server.Delivery != nil {
		t.Fatalf("the service completion after the stop is recorded as %+v", server)
	}
	if err := f.node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		turn, _, err := store.ScheduleBackground(t.Context(), db, f.node.ID(), f.session.ID, time.Now())
		if err == nil && turn != nil {
			t.Errorf("a completion after the stop admitted notice turn %+v", turn)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if runs, _, _ = f.read(); len(runs) != 1 {
		t.Fatalf("a completion after the stop started a turn: %+v", runs)
	}

	// The user's next message carries both outcomes and releases the hold on
	// the service that is still running.
	f.script.reset(func(int, map[string]string) [][3]string { return nil })
	f.submit("What happened?")
	runs, tasks = f.until("the next user turn", func(runs []protocol.ConversationRun, _ []protocol.ConversationBackgroundTask) bool {
		return len(runs) == 2 && slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool { return run.Status == protocol.ConversationStatusCompleted })
	})
	next := runs[slices.IndexFunc(runs, func(run protocol.ConversationRun) bool { return run.Status == protocol.ConversationStatusCompleted })]
	if next.Notice {
		t.Fatalf("the next turn is automatic: %+v", next)
	}
	for _, title := range []string{"go test ./...", "serve"} {
		if delivered := f.task(tasks, title); delivered.State != protocol.ConversationBackgroundStateDelivered || delivered.Delivery == nil || *delivered.Delivery != next.Turn {
			t.Errorf("%s is not delivered to the user's next turn: %+v", title, delivered)
		}
	}
	if watcher := f.task(tasks, "watch"); watcher.State != protocol.ConversationBackgroundStateRunning || watcher.Suppressed || watcher.Hold != nil {
		t.Errorf("the running service is still held after the user's next message: %+v", watcher)
	}
}

// TestSendNowEndsCommandWork interrupts a running turn with Send now, which
// stops its command work like a user stop and holds the result for the input
// that was sent.
func TestSendNowEndsCommandWork(t *testing.T) {
	blocking, testsStopped := make(chan struct{}), make(chan struct{})
	f := newStopFixture(t, func(ctx context.Context, launch process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		switch launch.Command {
		case "go test ./...":
			<-ctx.Done()
			close(testsStopped)
		case "block":
			close(blocking)
			<-ctx.Done()
		}
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
	})
	f.script.reset(func(step int, _ map[string]string) [][3]string {
		switch step {
		case 1:
			return [][3]string{{"tests", "run_command", `{"command":"go test ./...","background":true,"follow_up":true}`}}
		case 2:
			return [][3]string{{"block", "run_command", `{"command":"block"}`}}
		}
		return nil
	})
	f.submit("Run the tests")
	f.wait("the blocking foreground command", blocking)

	current := commandFixture(t, f.node, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: f.session.ID}}).Data.(protocol.Session)
	queued := commandFixture(t, f.node, protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{
		Session: f.session.ID, ExpectedRevision: current.Revision, Message: protocol.ConversationInput{Text: "Do this instead"},
	}}).Data.(protocol.QueuedTurn)
	_, _, queue := f.read()
	index := slices.IndexFunc(queue.Items, func(item protocol.ConversationPending) bool { return item.Turn == queued.ID })
	if index < 0 {
		t.Fatalf("queued input is missing from %+v", queue)
	}
	f.script.reset(func(int, map[string]string) [][3]string { return nil })
	commandFixture(t, f.node, protocol.Command{Kind: "send_queued_turn", Data: protocol.CommandSendQueuedTurnPayload{Turn: queued.ID, ExpectedRevision: queue.Items[index].Revision}})
	f.wait("the stop of the test command", testsStopped)
	_, tasks := f.until("the sent turn", func(runs []protocol.ConversationRun, tasks []protocol.ConversationBackgroundTask) bool {
		return slices.ContainsFunc(runs, func(run protocol.ConversationRun) bool {
			return run.Turn == queued.ID && run.Status == protocol.ConversationStatusCompleted
		}) && slices.ContainsFunc(tasks, func(task protocol.ConversationBackgroundTask) bool {
			return task.Title == "go test ./..." && task.State == protocol.ConversationBackgroundStateDelivered
		})
	})
	if tests := f.task(tasks, "go test ./..."); tests.Outcome == nil || *tests.Outcome != protocol.ConversationBackgroundOutcomeStopped || tests.Delivery == nil || *tests.Delivery != queued.ID {
		t.Fatalf("the stopped test command is recorded as %+v", tests)
	}
}

// TestStoppedOperationStopsLateCommand covers a background command started
// after a stop cancelled its turn's operation but before it was in the
// process table the stop scanned: it is stopped unless it is a service.
func TestStoppedOperationStopsLateCommand(t *testing.T) {
	stopped := map[string]chan struct{}{"late": make(chan struct{}), "service": make(chan struct{})}
	f := newStopFixture(t, func(ctx context.Context, launch process.Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		<-ctx.Done()
		close(stopped[launch.Command])
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
	})
	operation, cancel := context.WithCancel(t.Context())
	cancel()
	for _, command := range []string{"late", "service"} {
		launch := process.Launch{ID: protocol.NewRequestID(), Session: f.session.ID, Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: command,
			Access: protocol.ProcessAccessWorkspace, TimeoutMs: 60000, Background: true, Service: command == "service"}
		if _, err := f.node.startTurnCommand(operation, launch); err != nil {
			t.Fatal(err)
		}
	}
	f.wait("the stop of the late command", stopped["late"])
	select {
	case <-stopped["service"]:
		t.Fatal("the late service was stopped")
	case <-time.After(50 * time.Millisecond):
	}
}
