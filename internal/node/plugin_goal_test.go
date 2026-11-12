package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
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

// goalStep is one scripted model response: tool calls, or a sentence. A
// gated step waits until its gate closes or its turn is cancelled.
type goalStep struct {
	calls [][3]string
	text  string
	gate  chan struct{}
}

// goalScript answers the main session's model requests in order and keeps
// what each request was sent. A child session answers once its gate opens.
type goalScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	steps   []goalStep
	prompts []string
	results map[string]string
	extra   int
	main    atomic.Pointer[protocol.SessionID]
	child   chan struct{}
	// gated counts the requests that reached a gated step.
	gated atomic.Int32
}

func (script *goalScript) push(steps ...goalStep) {
	script.mu.Lock()
	script.steps = append(script.steps, steps...)
	script.mu.Unlock()
}

func (script *goalScript) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	var text strings.Builder
	script.mu.Lock()
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if value, found := fantasy.AsMessagePart[fantasy.TextPart](part); found {
				text.WriteString(value.Text)
				text.WriteByte('\n')
			}
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found {
				switch output := result.Output.(type) {
				case fantasy.ToolResultOutputContentText:
					script.results[result.ToolCallID] = output.Text
				case fantasy.ToolResultOutputContentError:
					script.results[result.ToolCallID] = output.Error.Error()
				}
			}
		}
	}
	script.prompts = append(script.prompts, text.String())
	if len(script.steps) == 0 {
		script.extra++
		script.mu.Unlock()
		return nil, errors.New("unexpected extra goal model request")
	}
	step := script.steps[0]
	script.steps = script.steps[1:]
	script.mu.Unlock()
	if step.gate != nil {
		script.gated.Add(1)
		select {
		case <-step.gate:
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}
	return func(yield func(fantasy.StreamPart) bool) {
		for _, call := range step.calls {
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: call[0], ToolCallName: call[1], ToolCallInput: call[2]}) {
				return
			}
		}
		if len(step.calls) == 0 && !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: step.text}) {
			return
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

// goalChild is a delegated session's model: one sentence after its gate.
type goalChild struct {
	fantasy.LanguageModel
	gate chan struct{}
}

func (child goalChild) Stream(ctx context.Context, _ fantasy.Call) (fantasy.StreamResponse, error) {
	select {
	case <-child.gate:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	return func(yield func(fantasy.StreamPart) bool) {
		_ = yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Child finished"}) &&
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

// goalFixture is a Node with the bundled goals package, a scripted model and
// one session.
type goalFixture struct {
	t         *testing.T
	ctx       context.Context
	profile   string
	node      *Node
	script    *goalScript
	session   protocol.Session
	reference protocol.PluginReference
	approved  int
}

func (f *goalFixture) start(script *goalScript) {
	f.t.Helper()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(f.ctx, f.profile, Options{Environment: &environment, Model: func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
		if main := script.main.Load(); main != nil && binding.Session != *main {
			return goalChild{gate: script.child}, nil
		}
		return script, nil
	}})
	if err != nil {
		f.t.Fatal(err)
	}
	f.node, f.script = node, script
	if err := node.profile.Store.Node.Read(f.ctx, func(db *sql.Conn) error {
		info, err := store.ReadPlugin(f.ctx, db, "goals")
		if err != nil || info == nil {
			return fmt.Errorf("the goals package is not bundled: %v", err)
		}
		f.reference = plugins.PackageReference(*info)
		return nil
	}); err != nil {
		f.t.Fatal(err)
	}
}

func newGoalScript() *goalScript {
	return &goalScript{results: map[string]string{}, child: make(chan struct{})}
}

func newGoalFixture(t *testing.T, mode protocol.WorkMode) *goalFixture {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 120*time.Second)
	t.Cleanup(cancel)
	f := &goalFixture{t: t, ctx: ctx, profile: filepath.Join(t.TempDir(), "profile")}
	f.start(newGoalScript())
	t.Cleanup(func() { f.node.Stop(context.Background()) })
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 200000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	if err := f.node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
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
	project := commandFixture(t, f.node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Goal", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", mode, protocol.PermissionAsk
	f.session = commandFixture(t, f.node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	f.script.main.Store(&f.session.ID)
	return f
}

func (f *goalFixture) current() protocol.Session {
	f.t.Helper()
	return commandFixture(f.t, f.node, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: f.session.ID}}).Data.(protocol.Session)
}

// send submits the user's message as the composer does.
func (f *goalFixture) send(text string) {
	f.t.Helper()
	commandFixture(f.t, f.node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: f.session.ID, ExpectedRevision: f.current().Revision, Message: protocol.ConversationInput{Text: text},
	}})
}

// call dispatches one of the package's handlers in the session's scope, as
// its panel does.
func (f *goalFixture) call(kind, handler string, input map[string]any) json.RawMessage {
	f.t.Helper()
	body, err := json.Marshal(input)
	if err != nil {
		f.t.Fatal(err)
	}
	command := protocol.Command{Kind: kind, Data: protocol.CommandCallPluginPayload{Handler: handler, Input: body}}
	if kind == "read_plugin_callback" {
		command.Data = protocol.CommandReadPluginCallbackPayload{Handler: handler, Input: body}
	}
	request := protocol.NewRequest(f.node.ID(), command)
	request.Plugin = &protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "workspace"}, Package: f.reference, Worktree: new(f.session.Worktree), Session: new(f.session.ID)}
	response := dispatchFixture(f.t, f.node, f.node.ID(), request)
	if response.Fault != nil {
		f.t.Fatalf("%s failed: %v", handler, response.Fault)
	}
	encoded, err := json.Marshal(response.Output.Data)
	if err != nil {
		f.t.Fatal(err)
	}
	return encoded
}

func (f *goalFixture) control(kind string) {
	f.t.Helper()
	_, goal := f.read()
	f.call("call_plugin", "control", map[string]any{"kind": kind, "goal_id": goal["id"]})
}

// read returns the session's runs and its stored goal, approving whatever
// waits for the user.
func (f *goalFixture) read() ([]protocol.ConversationRun, map[string]any) {
	f.t.Helper()
	var history protocol.ConversationHistory
	var goal map[string]any
	if err := f.node.profile.Store.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		if history, err = store.ReadConversation(f.ctx, db, f.session.ID, nil, 100); err != nil {
			return err
		}
		entry, err := store.ReadPluginConversationValue(f.ctx, db, f.reference, f.session.ID, "goal")
		if err != nil || !entry.Present {
			return err
		}
		return json.Unmarshal(entry.Value, &goal)
	}); err != nil {
		f.t.Fatal(err)
	}
	for _, approval := range history.Page.Approvals {
		if approval.State == protocol.ConversationApprovalStatePending {
			f.approved++
			commandFixture(f.t, f.node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: approval.Session, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"}}})
		}
	}
	return history.Page.Runs, goal
}

func (f *goalFixture) wait(name string, done func([]protocol.ConversationRun, map[string]any) bool) ([]protocol.ConversationRun, map[string]any) {
	f.t.Helper()
	deadline := time.Now().Add(30 * time.Second)
	for {
		runs, goal := f.read()
		if done(runs, goal) {
			return runs, goal
		}
		if time.Now().After(deadline) {
			statuses := []string{}
			for _, run := range runs {
				statuses = append(statuses, string(run.Status))
			}
			f.script.mu.Lock()
			extra, left := f.script.extra, len(f.script.steps)
			f.script.mu.Unlock()
			f.t.Fatalf("%s: timed out with runs %v, goal %v, %d unexpected and %d unused model steps", name, statuses, goal, extra, left)
		}
		time.Sleep(5 * time.Millisecond)
	}
}

// settled waits until the goal reaches the status and reason and the package
// has handled the session's last turn.
func (f *goalFixture) settled(status string, reason any) ([]protocol.ConversationRun, map[string]any) {
	f.t.Helper()
	return f.wait("goal "+status, func(runs []protocol.ConversationRun, goal map[string]any) bool {
		return goal != nil && goal["status"] == status && goal["reason"] == reason && goal["notice"] == nil
	})
}

// finished checks that the script was followed exactly.
func (f *goalFixture) finished() {
	f.t.Helper()
	f.script.mu.Lock()
	defer f.script.mu.Unlock()
	if f.script.extra != 0 || len(f.script.steps) != 0 {
		f.t.Fatalf("the model was asked %d unexpected times and %d scripted steps were unused", f.script.extra, len(f.script.steps))
	}
}

func (f *goalFixture) notices() []protocol.NotificationNotice {
	f.t.Helper()
	var notices []protocol.NotificationNotice
	for _, notice := range snapshotFixture(f.t, f.node).Notifications {
		if notice.Package == "goals" {
			notices = append(notices, notice)
		}
	}
	return notices
}

func (f *goalFixture) prompt(index int) string {
	f.script.mu.Lock()
	defer f.script.mu.Unlock()
	if index >= len(f.script.prompts) {
		f.t.Fatalf("the model was asked %d times, not %d", len(f.script.prompts), index+1)
	}
	return f.script.prompts[index]
}

func (f *goalFixture) result(call string) string {
	f.script.mu.Lock()
	defer f.script.mu.Unlock()
	return f.script.results[call]
}

// TestGoalLoop runs the bundled goals package on a real Node with a scripted
// model: the loop the package drives through after_turn and continue_turn,
// and every way it stops. Over 1 s: each case starts its own Node, which
// installs the bundled packages.
func TestGoalLoop(t *testing.T) {
	t.Parallel()
	update := operations.Alias("goals", "update_goal")
	work := [][3]string{{"work", "tool_search", `{"query":"select:` + update + `"}`}}
	block := func(note string) [][3]string {
		return [][3]string{{"block", update, `{"status":"blocked","note":"` + note + `"}`}}
	}
	// running waits until the model of the session's count-th turn is at
	// its gated step, the gated-th of the script.
	running := func(f *goalFixture, count int, gated int32) func([]protocol.ConversationRun, map[string]any) bool {
		return func(runs []protocol.ConversationRun, _ map[string]any) bool {
			return len(runs) == count && runs[count-1].Status == protocol.ConversationStatusRunning && f.script.gated.Load() == gated
		}
	}

	t.Run("continuation turns reach completion through claim and audit", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModeCode)
		f.script.push(
			goalStep{text: "Started"},
			goalStep{calls: work},
			goalStep{calls: [][3]string{{"claim", update, `{"status":"complete","note":"All checks pass"}`}}},
			goalStep{text: "Claimed"},
			goalStep{calls: [][3]string{{"confirm", update, `{"status":"complete","note":"Verified: all checks pass"}`}}},
			goalStep{text: "Verified"},
		)
		f.send("/goal Make the fixture pass and prove it")
		runs, goal := f.settled("completed", nil)
		f.finished()
		if len(runs) != 3 || runs[0].Continuation != nil || runs[1].Continuation == nil || runs[2].Continuation == nil {
			t.Fatalf("the goal ran %d turns: %+v", len(runs), runs)
		}
		for _, run := range runs {
			if run.Status != protocol.ConversationStatusCompleted {
				t.Fatalf("a goal turn ended %s: %+v", run.Status, run.Error)
			}
		}
		if work := runs[1].Continuation; work.Kind != nil || work.Marker == nil || work.Marker.Label != "Continue goal" || work.Limit == nil || *work.Limit != 25 || work.User {
			t.Fatalf("work continuation = %+v", work)
		}
		if audit := runs[2].Continuation; audit.Kind == nil || *audit.Kind != "verify" || audit.Marker == nil || audit.Marker.Label != "Verify goal" || audit.Marker.Locales["zh-CN"] != "核验目标" {
			t.Fatalf("audit continuation = %+v", audit)
		}
		if goal["evidence"] != "Verified: all checks pass" || goal["turns"] != float64(3) || goal["objective"] != "Make the fixture pass and prove it" {
			t.Fatalf("completed goal = %v", goal)
		}
		// The objective is pinned context, the continuation states the
		// budget and the audit carries the claim it must check.
		if first := f.prompt(0); !strings.Contains(first, "<objective>\nMake the fixture pass and prove it\n</objective>") || !strings.Contains(first, "Goal mode is active") {
			t.Fatalf("the first turn was not told the goal: %q", first)
		}
		if next := f.prompt(1); !strings.Contains(next, "Continue working toward the goal.\nThis is automatic turn 1 of at most 25 since the user's last message.") {
			t.Fatalf("continuation prompt = %q", next)
		}
		if audit := f.prompt(4); !strings.Contains(audit, "Verify the goal before it closes") || !strings.Contains(audit, "<claim>\nAll checks pass\n</claim>") {
			t.Fatalf("audit prompt = %q", audit)
		}
		if claim := f.result("claim"); !strings.Contains(claim, "verification turn follows") || !strings.Contains(claim, `"status":"active"`) {
			t.Fatalf("the claim closed the goal or was refused: %q", claim)
		}
		notices := f.notices()
		if len(notices) != 1 || notices[0].Content.Kind != protocol.NotificationKindSuccess || notices[0].Content.Presentation == nil || notices[0].Content.Presentation.Heading != "Goal completed" ||
			notices[0].Content.Title != "Make the fixture pass and prove it" || notices[0].Content.Message != "Verified: all checks pass" || notices[0].Content.Session == nil || *notices[0].Content.Session != f.session.ID {
			t.Fatalf("completion notices = %+v", notices)
		}
	})

	t.Run("a message during the goal runs inside it and the goal continues", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModeCode)
		gate := make(chan struct{})
		f.script.push(
			goalStep{text: "Started"},
			goalStep{gate: gate, calls: work},
			goalStep{text: "Step done"},
			goalStep{text: "Docs noted"},
			goalStep{calls: block("Need the docs repository")},
			goalStep{text: "Blocked"},
		)
		f.send("/goal Make the fixture pass and prove it")
		f.wait("the first continuation", running(f, 2, 1))
		f.send("Also update the docs")
		close(gate)
		runs, goal := f.settled("blocked", nil)
		f.finished()
		if len(runs) != 4 || runs[1].Continuation == nil || runs[2].Continuation != nil || runs[3].Continuation == nil {
			t.Fatalf("turns = %+v", runs)
		}
		if goal["detail"] != "Need the docs repository" || goal["turns"] != float64(4) || goal["objective"] != "Make the fixture pass and prove it" {
			t.Fatalf("goal after guidance = %v", goal)
		}
		// The user's turn ran as part of the goal and reset the unattended count.
		if inserted := f.prompt(3); !strings.Contains(inserted, "Also update the docs") || !strings.Contains(inserted, "Goal mode is active") {
			t.Fatalf("the inserted message did not run inside the goal: %q", inserted)
		}
		if next := f.prompt(4); !strings.Contains(next, "This is automatic turn 1 of at most 25") {
			t.Fatalf("the user's message did not reset the unattended count: %q", next)
		}
		if notices := f.notices(); len(notices) != 1 || notices[0].Content.Presentation.Heading != "Goal blocked" || notices[0].Content.Message != "Need the docs repository" {
			t.Fatalf("blocked notices = %+v", notices)
		}
	})

	t.Run("Stop pauses and a reply resumes; Pause holds across a reply", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModeCode)
		f.script.push(goalStep{text: "Started"}, goalStep{gate: make(chan struct{})})
		f.send("/goal Make the fixture pass and prove it")
		runs, _ := f.wait("the first continuation", running(f, 2, 1))
		commandFixture(t, f.node, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: runs[1].Turn}})
		runs, _ = f.settled("paused", "stopped")
		if runs[1].Status != protocol.ConversationStatusCancelled || len(f.notices()) != 0 {
			t.Fatalf("stopped turn = %s with notices %+v", runs[1].Status, f.notices())
		}
		// The reply runs first and the goal continues after it.
		f.script.push(goalStep{text: "On it"}, goalStep{gate: make(chan struct{})})
		f.send("Keep going")
		runs, goal := f.wait("the continuation after the reply", running(f, 4, 2))
		if runs[2].Continuation != nil || runs[3].Continuation == nil || goal["status"] != "active" {
			t.Fatalf("the reply did not resume the goal: %+v, %v", runs, goal)
		}
		// Pause stops the goal turn and holds the goal across a reply.
		f.control("pause")
		f.settled("paused", "user")
		f.wait("the paused goal turn", func(runs []protocol.ConversationRun, _ map[string]any) bool {
			return runs[3].Status == protocol.ConversationStatusCancelled
		})
		f.script.push(goalStep{text: "It is paused"})
		f.send("What is the status?")
		f.wait("the reply to a paused goal", func(runs []protocol.ConversationRun, _ map[string]any) bool {
			return len(runs) == 5 && runs[4].Status == protocol.ConversationStatusCompleted
		})
		// Resume is the user's request: one more continuation, the first of a new chain.
		f.script.push(goalStep{calls: block("Need a decision")}, goalStep{text: "Blocked"})
		f.control("resume")
		runs, goal = f.settled("blocked", nil)
		f.finished()
		if len(runs) != 6 || runs[4].Continuation != nil || runs[5].Continuation == nil || !runs[5].Continuation.User {
			t.Fatalf("the paused goal ran on its own or Resume did not continue it: %+v", runs)
		}
		if resumed := f.prompt(len(f.script.prompts) - 2); !strings.Contains(resumed, "The user resumed the goal.") || !strings.Contains(resumed, "This is automatic turn 1 of at most 25") {
			t.Fatalf("resume prompt = %q", resumed)
		}
		if goal["turns"] != float64(5) {
			t.Fatalf("goal counted %v turns; the reply to a paused goal is not a goal turn", goal["turns"])
		}
	})

	t.Run("the turn limit wraps up and pauses with its reason", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModeCode)
		f.script.push(goalStep{text: "Started"})
		for index := range 25 {
			f.script.push(goalStep{calls: work}, goalStep{text: fmt.Sprintf("Step %d", index+1)})
		}
		f.send("/goal Make the fixture pass and prove it")
		_, goal := f.settled("paused", "turns")
		f.finished()
		// The transcript page is bounded; the run list has every turn.
		page := commandFixture(t, f.node, protocol.Command{Kind: "read_conversation_runs", Data: protocol.CommandReadConversationRunsPayload{Session: f.session.ID, Limit: 100}}).Data.(protocol.ConversationRunsPage)
		if len(page.Runs) != 26 {
			t.Fatalf("the goal ran %d turns: %v", len(page.Runs), goal)
		}
		for _, run := range page.Runs {
			if run.Status != protocol.ConversationStatusCompleted {
				t.Fatalf("a goal turn ended %s", run.Status)
			}
		}
		if goal["chain"] != float64(25) || goal["turns"] != float64(26) {
			t.Fatalf("limited goal = %v", goal)
		}
		// Only the last allowed turn is told to wrap up.
		if last := f.prompt(49); !strings.Contains(last, "This is automatic turn 25 of at most 25") || !strings.Contains(last, "last automatic turn before the goal pauses, because the automatic turn limit is reached") {
			t.Fatalf("wrap-up prompt = %q", last)
		}
		if earlier := f.prompt(47); strings.Contains(earlier, "last automatic turn") {
			t.Fatalf("an earlier turn was told to wrap up: %q", earlier)
		}
		notices := f.notices()
		if len(notices) != 1 || notices[0].Content.Kind != protocol.NotificationKindWarning || notices[0].Content.Presentation.Heading != "Goal paused · Turn limit reached" || notices[0].Content.Presentation.Locales["zh-CN"] != "目标已暂停 · 已达轮次上限" {
			t.Fatalf("limit notices = %+v", notices)
		}
	})

	t.Run("the goal waits for background work and continues after its result", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModeCode)
		f.script.push(
			goalStep{calls: [][3]string{{"spawn", "spawn_agent", `{"task":"Check the fixture","background":true}`}}},
			goalStep{text: "Delegated"},
		)
		f.send("/goal Make the fixture pass and prove it")
		runs, goal := f.wait("the wait for background work", func(runs []protocol.ConversationRun, goal map[string]any) bool {
			return len(runs) == 1 && runs[0].Status == protocol.ConversationStatusCompleted && goal != nil && goal["waiting"] == true
		})
		// No turn is spent while the child runs.
		time.Sleep(200 * time.Millisecond)
		if runs, goal = f.read(); len(runs) != 1 || goal["status"] != "active" || goal["waiting"] != true {
			t.Fatalf("the goal did not wait: %+v, %v", runs, goal)
		}
		f.finished()
		f.script.push(goalStep{text: "The child is done"}, goalStep{calls: block("Need a review")}, goalStep{text: "Blocked"})
		close(f.script.child)
		runs, _ = f.settled("blocked", nil)
		f.finished()
		if len(runs) != 3 || !runs[1].Notice || len(runs[1].Notices) != 1 || runs[2].Continuation == nil || runs[2].Notice {
			t.Fatalf("turns after the result = %+v", runs)
		}
		if notice := f.prompt(2); !strings.Contains(notice, "Node background completion data follows") {
			t.Fatalf("the notice turn carried no result: %q", notice)
		}
		// The notice turn counts toward the goal's one limit.
		if next := f.prompt(3); !strings.Contains(next, "This is automatic turn 2 of at most 25") {
			t.Fatalf("continuation after the notice turn = %q", next)
		}
	})

	// shown is the goal as the panel reads it.
	shown := func(f *goalFixture) (status, reason, state string) {
		f.t.Helper()
		var view struct {
			Goal struct {
				Status string `json:"status"`
				Reason string `json:"reason"`
				State  string `json:"state"`
			} `json:"goal"`
		}
		if err := json.Unmarshal(f.call("read_plugin_callback", "inspect", map[string]any{}), &view); err != nil {
			f.t.Fatal(err)
		}
		return view.Goal.Status, view.Goal.Reason, view.Goal.State
	}
	// facts is what the Node states of the session's runs and background work.
	facts := func(f *goalFixture) protocol.ConversationRunsPage {
		f.t.Helper()
		return commandFixture(f.t, f.node, protocol.Command{Kind: "read_conversation_runs", Data: protocol.CommandReadConversationRunsPayload{Session: f.session.ID, Limit: 8}}).Data.(protocol.ConversationRunsPage)
	}
	// waiting starts a goal whose first turn delegates a background child
	// and leaves it waiting for the child's result.
	waiting := func(t *testing.T) *goalFixture {
		t.Helper()
		f := newGoalFixture(t, protocol.WorkModeCode)
		f.script.push(
			goalStep{calls: [][3]string{{"spawn", "spawn_agent", `{"task":"Check the fixture","background":true}`}}},
			goalStep{text: "Delegated"},
		)
		f.send("/goal Make the fixture pass and prove it")
		f.wait("the wait for background work", func(runs []protocol.ConversationRun, goal map[string]any) bool {
			return len(runs) == 1 && runs[0].Status == protocol.ConversationStatusCompleted && goal != nil && goal["waiting"] == true
		})
		if status, _, state := shown(f); status != "active" || state != "waiting" {
			t.Fatalf("a goal whose background work runs is shown %s, %s", status, state)
		}
		if page := facts(f); page.Background == nil || page.Background.Running != 1 || page.Background.Pending != 0 || len(page.Runs) != 1 || page.Runs[0].Recovered {
			t.Fatalf("run facts while the child runs = %+v, %+v", page.Background, page.Runs)
		}
		return f
	}

	t.Run("background work the user stops leaves the goal paused until a reply", func(t *testing.T) {
		t.Parallel()
		f := waiting(t)
		// The stopped child reports nothing by itself: no notice turn follows.
		stopped := false
		for _, session := range snapshotFixture(t, f.node).Sessions {
			if session.Delegation == nil || *session.Delegation == nil {
				continue
			}
			var history protocol.ConversationHistory
			if err := f.node.profile.Store.Node.Read(f.ctx, func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(f.ctx, db, session.ID, nil, 10)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, run := range history.Page.Runs {
				commandFixture(t, f.node, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: run.Turn}})
				stopped = true
			}
		}
		if !stopped {
			t.Fatal("the background child has no turn to stop")
		}
		deadline := time.Now().Add(30 * time.Second)
		for {
			status, reason, state := shown(f)
			if status == "paused" && reason == "stopped" && state == "paused" {
				break
			}
			if time.Now().After(deadline) {
				t.Fatalf("the goal whose background work was stopped is shown %s, %s, %s", status, reason, state)
			}
			time.Sleep(5 * time.Millisecond)
		}
		if page := facts(f); page.Background == nil || page.Background.Running != 0 || page.Background.Pending != 0 || page.Runs[0].Recovered {
			t.Fatalf("run facts after the stop = %+v, %+v", page.Background, page.Runs)
		}
		// Nothing was started for it, and the stored goal is what a reply continues.
		if runs, goal := f.read(); len(runs) != 1 || goal["status"] != "active" {
			t.Fatalf("after the stop: %+v, %v", runs, goal)
		}
		f.finished()
		f.script.push(goalStep{text: "Going on without it"}, goalStep{calls: block("Need a review")}, goalStep{text: "Blocked"})
		f.send("Carry on without the check")
		runs, _ := f.settled("blocked", nil)
		f.finished()
		if len(runs) != 3 || runs[1].Notice || runs[1].Continuation != nil || runs[2].Continuation == nil {
			t.Fatalf("turns after the reply = %+v", runs)
		}
	})

	t.Run("a restart while the goal waits for background work leaves it paused", func(t *testing.T) {
		t.Parallel()
		f := waiting(t)
		if err := f.node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		script := newGoalScript()
		script.main.Store(&f.session.ID)
		f.start(script)
		// The goal's last turn completed and was handled; it ended before
		// this Node started, so nothing of its loop is left.
		if page := facts(f); len(page.Runs) != 1 || page.Runs[0].Status != protocol.ConversationStatusCompleted || !page.Runs[0].Recovered ||
			page.Background == nil || page.Background.Running != 0 || page.Background.Pending != 0 {
			t.Fatalf("run facts after the restart = %+v, %+v", page.Background, page.Runs)
		}
		if status, reason, state := shown(f); status != "paused" || reason != "restart" || state != "paused" {
			t.Fatalf("goal after the restart = %s, %s, %s", status, reason, state)
		}
		time.Sleep(300 * time.Millisecond)
		if runs, _ := f.read(); len(runs) != 1 {
			t.Fatalf("turns after the restart = %+v", runs)
		}
		f.finished()
		f.script.push(goalStep{calls: block("Need a decision")}, goalStep{text: "Blocked"})
		f.control("resume")
		runs, _ := f.settled("blocked", nil)
		f.finished()
		if len(runs) != 2 || runs[1].Continuation == nil || !runs[1].Continuation.User {
			t.Fatalf("Resume after the restart = %+v", runs)
		}
	})

	t.Run("a restart leaves the goal paused until Resume", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModeCode)
		f.script.push(goalStep{text: "Started"}, goalStep{gate: make(chan struct{})})
		f.send("/goal Make the fixture pass and prove it")
		f.wait("the first continuation", running(f, 2, 1))
		if err := f.node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		script := newGoalScript()
		script.main.Store(&f.session.ID)
		f.start(script)
		var shown struct {
			Goal struct {
				Status string `json:"status"`
				Reason string `json:"reason"`
				State  string `json:"state"`
			} `json:"goal"`
		}
		if err := json.Unmarshal(f.call("read_plugin_callback", "inspect", map[string]any{}), &shown); err != nil {
			t.Fatal(err)
		}
		if shown.Goal.Status != "paused" || shown.Goal.Reason != "restart" || shown.Goal.State != "paused" {
			t.Fatalf("goal after the restart = %+v", shown.Goal)
		}
		// Nothing resumes by itself.
		time.Sleep(300 * time.Millisecond)
		runs, _ := f.read()
		if len(runs) != 2 || runs[1].Status != protocol.ConversationStatusInterrupted {
			t.Fatalf("turns after the restart = %+v", runs)
		}
		f.finished()
		f.script.push(goalStep{calls: block("Need a decision")}, goalStep{text: "Blocked"})
		f.control("resume")
		runs, _ = f.settled("blocked", nil)
		f.finished()
		if len(runs) != 3 || runs[2].Continuation == nil || !runs[2].Continuation.User {
			t.Fatalf("Resume after the restart = %+v", runs)
		}
	})

	t.Run("the model cannot pause, resume, edit or clear on its own", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModeCode)
		f.script.push(
			goalStep{text: "Started"},
			goalStep{calls: [][3]string{
				{"pause", update, `{"status":"paused"}`},
				{"resume", update, `{"status":"active"}`},
				{"edit", update, `{"status":"blocked","note":"Shrink it","objective":"A smaller goal"}`},
				{"clear", operations.Alias("goals", "clear_goal"), `{}`},
			}},
			goalStep{calls: block("Only the user can decide")},
			goalStep{text: "Blocked"},
		)
		f.send("/goal Make the fixture pass and prove it")
		_, goal := f.settled("blocked", nil)
		f.finished()
		for call, refusal := range map[string]string{"pause": "Only the user can pause the goal", "resume": "Only the user can resume the goal", "edit": "invalid tool arguments", "clear": "not handled"} {
			if result := f.result(call); !strings.Contains(result, refusal) {
				t.Errorf("%s was not refused with %q: %q", call, refusal, result)
			}
		}
		if goal["objective"] != "Make the fixture pass and prove it" || goal["version"] != float64(1) {
			t.Fatalf("the model changed the goal: %v", goal)
		}
	})

	t.Run("/goal in Plan mode plans first and pursues once a turn leaves Plan", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModePlan)
		f.script.push(goalStep{text: "Here is the plan"})
		f.send("/goal Make the fixture pass and prove it")
		f.wait("the planning turn", func(runs []protocol.ConversationRun, goal map[string]any) bool {
			return len(runs) == 1 && runs[0].Status == protocol.ConversationStatusCompleted && goal != nil && goal["status"] == "paused" && goal["reason"] == "plan"
		})
		time.Sleep(200 * time.Millisecond)
		if runs, goal := f.read(); len(runs) != 1 || goal["status"] != "paused" {
			t.Fatalf("the goal ran in Plan mode: %+v, %v", runs, goal)
		}
		if plan := f.prompt(0); !strings.Contains(plan, "The session is in Plan mode: plan how to achieve the objective") || !strings.Contains(plan, "<objective>") {
			t.Fatalf("the planning turn was not told the goal: %q", plan)
		}
		session := f.current()
		config := session.Config
		config.Mode = protocol.WorkModeCode
		commandFixture(t, f.node, protocol.Command{Kind: "set_session_config", Data: protocol.CommandSetSessionConfigPayload{Session: session.ID, ExpectedRevision: session.Revision, Config: config}})
		f.script.push(goalStep{text: "Starting"}, goalStep{calls: block("Need a decision")}, goalStep{text: "Blocked"})
		f.send("Go ahead")
		runs, _ := f.settled("blocked", nil)
		f.finished()
		if len(runs) != 3 || runs[1].Continuation != nil || runs[2].Continuation == nil {
			t.Fatalf("turns after leaving Plan = %+v", runs)
		}
	})

	t.Run("create_goal asks the user and then starts the loop", func(t *testing.T) {
		t.Parallel()
		f := newGoalFixture(t, protocol.WorkModeCode)
		f.script.push(
			goalStep{calls: [][3]string{{"create", operations.Alias("goals", "create_goal"), `{"objective":"Ship the fixture"}`}}},
			goalStep{text: "Goal set"},
			goalStep{calls: block("Need a decision")},
			goalStep{text: "Blocked"},
		)
		f.send("Please set a goal to ship the fixture")
		runs, goal := f.settled("blocked", nil)
		f.finished()
		if f.approved != 1 {
			t.Fatalf("create_goal asked %d times", f.approved)
		}
		if len(runs) != 2 || runs[1].Continuation == nil || goal["objective"] != "Ship the fixture" {
			t.Fatalf("goal started by the model = %+v, %v", runs, goal)
		}
	})
}
