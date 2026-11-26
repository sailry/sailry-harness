package scheduledtasksdesktop

import (
	"context"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type fakeTasksHarness struct {
	mu           sync.Mutex
	localNode    protocol.NodeID
	remoteNode   protocol.NodeID
	nodes        []viewapi.NodeInfo
	snapshots    map[protocol.NodeID]*protocol.Snapshot
	executeFn    func(ctx context.Context, node protocol.NodeID, req protocol.Request) (protocol.Output, error)
	settleFn     func(ctx context.Context, node protocol.NodeID, req protocol.Request) (protocol.Output, error)
	resolveFn    func(ctx context.Context, node protocol.NodeID, req protocol.Request) (viewapi.Receipt, error)
	settledReqs  []protocol.Request
	resolvedReqs []protocol.Request
	openedNode   protocol.NodeID
	openedSess   protocol.SessionID
	clipboard    string
	revealCount  atomic.Int32
	unsubCount   atomic.Int32
	wakeCount    atomic.Int32
	changeFn     func()
}

func newFakeTasksHarness() (*fakeTasksHarness, protocol.ProjectID, protocol.ProviderID, protocol.TeamID) {
	localNode := protocol.NodeID{1}
	remoteNode := protocol.NodeID{2}
	projID := protocol.NewProjectID()
	provID := protocol.NewProviderID()
	teamID := protocol.NewTeamID()
	leadEffort := protocol.Effort{Kind: "high"}

	localSnap := protocol.DefaultSnapshot()
	localSnap.Node = localNode
	localSnap.Projects = []protocol.Project{{ID: projID, Name: "Core App", Path: "/repo/core"}}
	localSnap.Providers = []protocol.ConversationProvider{{
		ID:      provID,
		Name:    "Anthropic",
		Api:     protocol.ConversationModelApiAnthropic,
		Enabled: true,
		Models: []protocol.ConversationModel{
			{ID: "claude-sonnet", Reasoning: true, Efforts: []protocol.Effort{{Kind: "low"}, {Kind: "high"}}, DefaultEffort: protocol.Effort{Kind: "low"}},
			{ID: "claude-opus", Reasoning: true, Efforts: []protocol.Effort{{Kind: "high"}}, DefaultEffort: protocol.Effort{Kind: "high"}},
		},
	}}
	localSnap.Teams = []protocol.TeamProfile{{
		ID:   teamID,
		Name: "Platform Team",
		Model: &protocol.RoleModel{
			Provider: provID,
			Model:    "claude-opus",
			Effort:   &leadEffort,
		},
	}}

	remoteSnap := protocol.DefaultSnapshot()
	remoteSnap.Node = remoteNode

	h := &fakeTasksHarness{
		localNode:  localNode,
		remoteNode: remoteNode,
		nodes: []viewapi.NodeInfo{
			{ID: localNode, Label: "Local Mac", Connected: true, Local: true},
			{ID: remoteNode, Label: "Build Server", Connected: true, Local: false},
		},
		snapshots: map[protocol.NodeID]*protocol.Snapshot{
			localNode:  &localSnap,
			remoteNode: &remoteSnap,
		},
	}
	return h, projID, provID, teamID
}

func (h *fakeTasksHarness) services() viewapi.Services {
	return viewapi.Services{
		Node:      h.localNode,
		Connected: func() bool { return true },
		Wake:      func() { h.wakeCount.Add(1) },
		Async: func(work func()) bool {
			go work()
			return true
		},
		Nodes: func() []viewapi.NodeInfo {
			h.mu.Lock()
			defer h.mu.Unlock()
			out := make([]viewapi.NodeInfo, len(h.nodes))
			copy(out, h.nodes)
			return out
		},
		Snapshot: func(node protocol.NodeID) (*protocol.Snapshot, bool) {
			h.mu.Lock()
			defer h.mu.Unlock()
			snap, ok := h.snapshots[node]
			return snap, ok
		},
		PrepareOn: func(node protocol.NodeID, cmd protocol.Command) (protocol.Request, error) {
			return protocol.Request{
				Version: protocol.VERSION,
				ID:      protocol.NewRequestID(),
				Target:  node,
				Command: cmd,
			}, nil
		},
		ExecuteOn: func(ctx context.Context, node protocol.NodeID, req protocol.Request) (protocol.Output, error) {
			h.mu.Lock()
			fn := h.executeFn
			h.mu.Unlock()
			if fn != nil {
				return fn(ctx, node, req)
			}
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "unhandled"}
		},
		SettleOn: func(ctx context.Context, node protocol.NodeID, req protocol.Request) (protocol.Output, error) {
			h.mu.Lock()
			h.settledReqs = append(h.settledReqs, req)
			fn := h.settleFn
			h.mu.Unlock()
			if fn != nil {
				return fn(ctx, node, req)
			}
			return protocol.Output{}, nil
		},
		ResolveOn: func(ctx context.Context, node protocol.NodeID, req protocol.Request) (viewapi.Receipt, error) {
			h.mu.Lock()
			h.resolvedReqs = append(h.resolvedReqs, req)
			fn := h.resolveFn
			h.mu.Unlock()
			if fn != nil {
				return fn(ctx, node, req)
			}
			return viewapi.Receipt{State: "absent"}, nil
		},
		OpenSession: func(node protocol.NodeID, id protocol.SessionID) {
			h.mu.Lock()
			h.openedNode = node
			h.openedSess = id
			h.mu.Unlock()
		},
		RevealDetails: func() { h.revealCount.Add(1) },
		WriteClipboard: func(text string) error {
			h.mu.Lock()
			h.clipboard = text
			h.mu.Unlock()
			return nil
		},
		OnChange: func(cb func()) func() {
			h.mu.Lock()
			h.changeFn = cb
			h.mu.Unlock()
			var once sync.Once
			return func() {
				once.Do(func() { h.unsubCount.Add(1) })
			}
		},
	}
}

func waitFor(t *testing.T, cond func() bool, desc string) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		if cond() {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", desc)
}

func TestRegisterAndLocales(t *testing.T) {
	assembly := shell.Assemble(Register)
	if len(assembly.WorkspacePages) != 1 {
		t.Fatalf("WorkspacePages count = %d, want 1", len(assembly.WorkspacePages))
	}
	p := assembly.WorkspacePages[0]
	if p.Page != PageID || p.Feature.ID != "page:scheduled_tasks" || p.Feature.Icon != icons.CalendarNav ||
		!p.Feature.DefaultPinned || p.Feature.LegacyPackage != "scheduled-tasks" || !p.ReplacesLegacyViews {
		t.Fatalf("unexpected NativePage registration: %+v", p)
	}
	cat := i18n.New(assembly.Locales...)
	if got := cat.In("en", p.Feature.Label); got != "Scheduled tasks" {
		t.Fatalf("en label = %q", got)
	}
	if got := cat.In("zh-CN", p.Feature.Label); got != "定时任务" {
		t.Fatalf("zh-CN label = %q", got)
	}
}

func TestControllerCRUDTeamLeadAndMultiNode(t *testing.T) {
	h, projID, provID, teamID := newFakeTasksHarness()
	fixedNow := time.Date(2026, 4, 6, 10, 0, 0, 0, time.UTC)
	rev := uint64(3)
	nextLocal := fixedNow.Add(time.Hour).UnixMilli()
	nextRemote := fixedNow.Add(2 * time.Hour).UnixMilli()
	started := fixedNow.Add(-5 * time.Minute).UnixMilli()
	sessID := protocol.NewSessionID()
	turnID := protocol.NewTurnID()
	jobQueued := protocol.NewJobID()
	jobRunning := protocol.NewJobID()

	localTask := protocol.ScheduledTask{
		ID:            "11111111-1111-7111-8111-111111111111",
		ExecutionNode: &h.localNode,
		Name:          "Morning Digest",
		Prompt:        "Summarize overnight activity",
		Project:       &projID,
		Config:        protocol.SessionConfig{Provider: provID, Model: "claude-sonnet", Effort: protocol.Effort{Kind: "low"}, Mode: protocol.WorkModeCode, Permission: protocol.PermissionAsk},
		Execution:     protocol.ConversationExecutionModel,
		Timing:        protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{Timezone: "UTC", Weekdays: protocol.Bytes{1, 2, 3, 4, 5}, Hour: 9, Minute: 0}},
		Enabled:       true,
		Queue:         "default",
	}
	remoteTask := protocol.ScheduledTask{
		ID:            "22222222-2222-7222-8222-222222222222",
		ExecutionNode: &h.remoteNode,
		Name:          "Remote Nightly",
		Prompt:        "Run integration checks",
		Config:        protocol.SessionConfig{Provider: provID, Model: "claude-sonnet", Effort: protocol.Effort{Kind: "low"}, Mode: protocol.WorkModeCode, Permission: protocol.PermissionAsk},
		Execution:     protocol.ConversationExecutionModel,
		Timing:        protocol.DispatchTiming{Kind: "every", Data: protocol.DispatchTimingEveryPayload{AnchorMs: fixedNow.UnixMilli(), IntervalMs: HourMs}},
		Enabled:       true,
		Queue:         "default",
	}

	h.executeFn = func(_ context.Context, node protocol.NodeID, req protocol.Request) (protocol.Output, error) {
		switch req.Command.Kind {
		case "list_scheduled_tasks":
			return protocol.Output{Kind: "scheduled_task_page", Data: protocol.ScheduledTaskPage{
				Items: []protocol.ScheduledTaskEntry{
					{Task: localTask, Revision: &rev, NextMs: &nextLocal},
					{Task: remoteTask, Revision: &rev},
				},
			}}, nil
		case "read_scheduled_task_runtime":
			if node == h.localNode {
				return protocol.Output{Kind: "scheduled_task_runtime", Data: protocol.ScheduledTaskRuntime{
					Status: map[string]protocol.ScheduledTaskRunState{
						localTask.ID: {Status: protocol.DispatchStatusRunning, StartedMs: &started},
					},
				}}, nil
			}
			return protocol.Output{Kind: "scheduled_task_runtime", Data: protocol.ScheduledTaskRuntime{
				Status: map[string]protocol.ScheduledTaskRunState{
					remoteTask.ID: {Status: protocol.DispatchStatusCompleted},
				},
				Schedules: []protocol.DispatchSchedule{{Topic: remoteTask.ID, NextMs: &nextRemote}},
			}}, nil
		case "read_scheduled_task_history":
			return protocol.Output{Kind: "scheduled_task_history", Data: protocol.ScheduledTaskHistory{
				Items: []protocol.ScheduledTaskHistoryEntry{
					{Job: protocol.DispatchJob{ID: jobRunning, Handler: localTask.ID, Status: protocol.DispatchStatusRunning, CreatedMs: started, StartedMs: &started}, ExecutionNode: h.localNode, Session: &sessID, Turn: &turnID},
					{Job: protocol.DispatchJob{ID: jobQueued, Handler: localTask.ID, Status: protocol.DispatchStatusQueued, CreatedMs: started - MinuteMs}, ExecutionNode: h.localNode},
				},
			}}, nil
		}
		return protocol.Output{}, nil
	}

	ctrl := newController(t.Context(), h.services(), func() bool { return true })
	ctrl.nowFn = func() time.Time { return fixedNow }
	ctrl.loc = time.UTC
	defer ctrl.close()

	ctrl.sync()
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return ctrl.loaded && !ctrl.loading && len(ctrl.items) == 2
	}, "initial multi-node load")

	ctrl.mu.Lock()
	if ctrl.items[0].Status != protocol.DispatchStatusRunning {
		t.Fatalf("local status = %q, want running", ctrl.items[0].Status)
	}
	if ctrl.items[1].NextMs == nil || *ctrl.items[1].NextMs != nextRemote {
		t.Fatalf("remote NextMs = %v, want %d", ctrl.items[1].NextMs, nextRemote)
	}
	ctrl.mu.Unlock()

	// New draft defaults to "model" execution; switching to team with fixed lead resolves lead model.
	ctrl.openEditor(nil)
	ctrl.mu.Lock()
	ed := ctrl.editor
	if ed == nil || ed.execution != protocol.ConversationExecutionModel {
		t.Fatalf("new draft execution = %v, want model", ed)
	}
	ed.name = "Team Task"
	ed.prompt = "Check releases"
	ed.execution = protocol.ConversationExecutionTeam
	ed.team = &protocol.TeamSelection{ID: teamID, Revision: nil}
	ctrl.syncEditorLeadModelLocked(ed)
	if ed.config == nil || ed.config.Model != "claude-opus" || effortString(ed.config.Effort) != "high" {
		t.Fatalf("fixed team lead config = %+v", ed.config)
	}
	ctrl.mu.Unlock()

	ctrl.saveEditor()
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy && ctrl.editor == nil
	}, "save completion")

	h.mu.Lock()
	if len(h.settledReqs) != 1 || h.settledReqs[0].Command.Kind != "save_scheduled_task" {
		t.Fatalf("settled reqs = %+v", h.settledReqs)
	}
	savePayload := h.settledReqs[0].Command.Data.(*protocol.CommandSaveScheduledTaskPayload)
	if savePayload.Task.Config.Model != "claude-opus" || savePayload.Task.Team == nil || savePayload.Task.Team.Revision != nil {
		t.Fatalf("unexpected save payload: %+v", savePayload.Task)
	}
	h.mu.Unlock()

	// Inspect details & records, then stop running job and cancel queued job.
	ctrl.inspect(ctrl.items[0], "records")
	if h.revealCount.Load() != 1 {
		t.Fatalf("RevealDetails calls = %d, want 1", h.revealCount.Load())
	}
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return ctrl.historyLoaded && len(ctrl.history) == 2
	}, "history loaded")

	ctrl.cancelOrStopRun(ctrl.history[0], h.localNode)
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy
	}, "stop job")
	ctrl.cancelOrStopRun(ctrl.history[1], h.localNode)
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy
	}, "cancel job")

	h.mu.Lock()
	if len(h.settledReqs) != 3 || h.settledReqs[1].Command.Kind != "stop_scheduled_task_job" || h.settledReqs[2].Command.Kind != "cancel_scheduled_task_job" {
		t.Fatalf("unexpected job control requests: %+v", h.settledReqs)
	}
	h.mu.Unlock()
}

func TestControllerUncertainMutationRecoveryReusesRequestID(t *testing.T) {
	h, _, provID, _ := newFakeTasksHarness()
	fixedNow := time.Date(2026, 4, 6, 10, 0, 0, 0, time.UTC)
	rev := uint64(1)
	task := protocol.ScheduledTask{
		ID:            "33333333-3333-7333-8333-333333333333",
		ExecutionNode: &h.localNode,
		Name:          "Deploy Check",
		Prompt:        "Verify health",
		Config:        protocol.SessionConfig{Provider: provID, Model: "claude-sonnet", Effort: protocol.Effort{Kind: "low"}, Mode: protocol.WorkModeCode, Permission: protocol.PermissionAsk},
		Execution:     protocol.ConversationExecutionModel,
		Timing:        protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: fixedNow.Add(time.Hour).UnixMilli()}},
		Enabled:       true,
		Queue:         "default",
	}

	h.executeFn = func(_ context.Context, _ protocol.NodeID, req protocol.Request) (protocol.Output, error) {
		switch req.Command.Kind {
		case "list_scheduled_tasks":
			return protocol.Output{Kind: "scheduled_task_page", Data: protocol.ScheduledTaskPage{
				Items: []protocol.ScheduledTaskEntry{{Task: task, Revision: &rev}},
			}}, nil
		case "read_scheduled_task_runtime":
			return protocol.Output{Kind: "scheduled_task_runtime", Data: protocol.ScheduledTaskRuntime{}}, nil
		}
		return protocol.Output{}, nil
	}

	var attempts atomic.Int32
	h.settleFn = func(_ context.Context, _ protocol.NodeID, _ protocol.Request) (protocol.Output, error) {
		if attempts.Add(1) == 1 {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "connection dropped"}
		}
		return protocol.Output{Kind: "scheduled_task_job"}, nil
	}
	h.resolveFn = func(_ context.Context, _ protocol.NodeID, _ protocol.Request) (viewapi.Receipt, error) {
		return viewapi.Receipt{State: "replayable"}, nil
	}

	ctrl := newController(t.Context(), h.services(), func() bool { return true })
	ctrl.nowFn = func() time.Time { return fixedNow }
	defer ctrl.close()

	ctrl.sync()
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return ctrl.loaded && !ctrl.loading
	}, "loaded")

	ctrl.runNow(ctrl.items[0])
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy && ctrl.pending != nil
	}, "uncertain pending state")

	ctrl.mu.Lock()
	origReqID := ctrl.pending.request.ID
	ctrl.mu.Unlock()

	ctrl.retryPending()
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy && ctrl.pending == nil
	}, "retry resolved and succeeded")

	h.mu.Lock()
	if len(h.resolvedReqs) != 1 || h.resolvedReqs[0].ID != origReqID {
		t.Fatalf("ResolveOn req IDs = %+v, want %v", h.resolvedReqs, origReqID)
	}
	if len(h.settledReqs) != 2 || h.settledReqs[0].ID != origReqID || h.settledReqs[1].ID != origReqID {
		t.Fatalf("SettleOn did not reuse exact original RequestID: %+v", h.settledReqs)
	}
	h.mu.Unlock()

	// Dismissing an uncertain mutation clears pending without repeating under a new ID.
	attempts.Store(0)
	ctrl.runNow(ctrl.items[0])
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy && ctrl.pending != nil
	}, "second uncertain state")
	ctrl.dismissPending()
	ctrl.mu.Lock()
	if ctrl.pending != nil {
		t.Fatal("dismissPending did not clear pending")
	}
	ctrl.mu.Unlock()
}

func TestControllerCloseAndStaleReplyDiscard(t *testing.T) {
	h, _, _, _ := newFakeTasksHarness()
	release := make(chan struct{})
	started := make(chan struct{}, 1)
	h.executeFn = func(_ context.Context, _ protocol.NodeID, req protocol.Request) (protocol.Output, error) {
		if req.Command.Kind == "list_scheduled_tasks" {
			select {
			case started <- struct{}{}:
			default:
			}
			<-release
			return protocol.Output{Kind: "scheduled_task_page", Data: protocol.ScheduledTaskPage{
				Items: []protocol.ScheduledTaskEntry{{Task: protocol.ScheduledTask{ID: "late", Name: "Stale"}}},
			}}, nil
		}
		return protocol.Output{Kind: "scheduled_task_runtime", Data: protocol.ScheduledTaskRuntime{}}, nil
	}

	ctrl := newController(t.Context(), h.services(), func() bool { return true })
	ctrl.sync()
	<-started

	// Closing cancels context, unsubscribes once, and discards late reply.
	ctrl.close()
	ctrl.close()
	if h.unsubCount.Load() != 1 {
		t.Fatalf("OnChange unsubscribe count = %d, want 1", h.unsubCount.Load())
	}
	if ctrl.ctx.Err() == nil {
		t.Fatal("controller context was not cancelled on close")
	}
	close(release)
	time.Sleep(30 * time.Millisecond)

	ctrl.mu.Lock()
	if len(ctrl.items) != 0 {
		t.Fatalf("late reply mutated closed controller: %+v", ctrl.items)
	}
	ctrl.mu.Unlock()
}

func TestScheduledTasksUIAndDetailsAndDeleteConfirm(t *testing.T) {
	h, projID, provID, _ := newFakeTasksHarness()
	fixedNow := time.Date(2026, 4, 6, 10, 0, 0, 0, time.UTC)
	rev := uint64(2)
	nextMs := fixedNow.Add(time.Hour).UnixMilli()
	failAt := fixedNow.Add(-10 * time.Minute).UnixMilli()
	sessID := protocol.NewSessionID()

	task := protocol.ScheduledTask{
		ID:            "44444444-4444-7444-8444-444444444444",
		ExecutionNode: &h.localNode,
		Name:          "Weekly Audit",
		Prompt:        "Check all security alerts",
		Project:       &projID,
		Config:        protocol.SessionConfig{Provider: provID, Model: "claude-sonnet", Effort: protocol.Effort{Kind: "low"}, Mode: protocol.WorkModeCode, Permission: protocol.PermissionAsk},
		Execution:     protocol.ConversationExecutionModel,
		Timing:        protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{Timezone: "UTC", Weekdays: protocol.Bytes{1, 3, 5}, Hour: 9, Minute: 0}},
		Enabled:       true,
		Queue:         "default",
	}

	h.executeFn = func(_ context.Context, _ protocol.NodeID, req protocol.Request) (protocol.Output, error) {
		switch req.Command.Kind {
		case "list_scheduled_tasks":
			return protocol.Output{Kind: "scheduled_task_page", Data: protocol.ScheduledTaskPage{
				Items: []protocol.ScheduledTaskEntry{{Task: task, Revision: &rev, NextMs: &nextMs}},
			}}, nil
		case "read_scheduled_task_runtime":
			return protocol.Output{Kind: "scheduled_task_runtime", Data: protocol.ScheduledTaskRuntime{
				Status: map[string]protocol.ScheduledTaskRunState{task.ID: {Status: protocol.DispatchStatusFailed}},
			}}, nil
		case "read_scheduled_task_history":
			return protocol.Output{Kind: "scheduled_task_history", Data: protocol.ScheduledTaskHistory{
				Items: []protocol.ScheduledTaskHistoryEntry{
					{Job: protocol.DispatchJob{ID: protocol.NewJobID(), Handler: task.ID, Status: protocol.DispatchStatusFailed, CreatedMs: failAt, Error: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "upstream timeout"}}, ExecutionNode: h.localNode, Session: &sessID},
					{Job: protocol.DispatchJob{ID: protocol.NewJobID(), Handler: task.ID, Status: protocol.DispatchStatusFailed, CreatedMs: failAt - HourMs}, ExecutionNode: h.localNode},
				},
			}}, nil
		}
		return protocol.Output{}, nil
	}

	assembly := shell.Assemble(Register)
	app := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
	svc := h.services()
	svc.RevealDetails = func() {
		h.revealCount.Add(1)
		app.RevealDetails()
	}
	app.BindWindow(viewapi.WithServices(t.Context(), svc))
	defer app.CloseWindowBindings()
	app.Navigate(PageID)
	app.SyncNativePages()

	var confirms []kit.Confirmation
	var confirmAnswer func(int)
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		kit.BindConfirmations(c, func(req kit.Confirmation, answer func(int)) {
			confirms = append(confirms, req)
			confirmAnswer = answer
		})
		app.View(c)
	}, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tester.SetLocale("en")

	waitFor(t, func() bool {
		tester.Frame()
		return tester.HasText("Weekly Audit")
	}, "task card rendered")

	clickEnabledButton := func(label string) {
		t.Helper()
		for _, node := range tester.AccessAll() {
			if node.Role == ui.RoleButton && node.Name == label && !node.Disabled {
				tester.ClickAt(node.Bounds.X+node.Bounds.W/2, node.Bounds.Y+node.Bounds.H/2)
				tester.Frame()
				return
			}
		}
		t.Fatalf("enabled button %q not found; texts=%v", label, tester.Texts())
	}

	// Open Details right column.
	clickEnabledButton("Details")
	waitFor(t, func() bool {
		tester.Frame()
		return tester.HasText("Last run failed") && tester.HasText("Check all security alerts")
	}, "details column with streak card rendered")

	// Verify streak text is present in the details panel.
	foundStreak := false
	for _, txt := range tester.Texts() {
		if len(txt) >= len("2 failures in a row") && (txt == "2 failures in a row" || containsSubstr(txt, "2 failures in a row")) {
			foundStreak = true
			break
		}
	}
	if !foundStreak {
		t.Fatalf("expected consecutive failure streak text, got %v", tester.Texts())
	}

	// Copy prompt button writes prompt to clipboard.
	clickEnabledButton("Copy")
	h.mu.Lock()
	copied := h.clipboard
	h.mu.Unlock()
	if copied != task.Prompt {
		t.Fatalf("clipboard = %q, want %q", copied, task.Prompt)
	}

	// Switch to Records tab and open session.
	clickEnabledButton("Records")
	waitFor(t, func() bool {
		tester.Frame()
		return tester.HasText("upstream timeout")
	}, "records timeline rendered")
	clickEnabledButton("Open session")
	h.mu.Lock()
	gotSess, gotNode := h.openedSess, h.openedNode
	h.mu.Unlock()
	if gotSess != sessID || gotNode != h.localNode {
		t.Fatalf("OpenSession = (%v, %v), want (%v, %v)", gotNode, gotSess, h.localNode, sessID)
	}

	// Open New task editor dialog, trigger concurrent OnChange while rendering, and close cleanly.
	clickEnabledButton("New task")
	if !tester.HasText("Name") || !tester.HasText("Schedule") || !tester.HasText("Execution") {
		t.Fatalf("editor dialog did not render expected fields: %v", tester.Texts())
	}
	h.mu.Lock()
	cb := h.changeFn
	h.mu.Unlock()
	if cb != nil {
		go cb()
	}
	tester.Frame()
	clickEnabledButton("Cancel")

	// Trigger OS-native delete confirmation and confirm.
	clickEnabledButton("Delete")
	if len(confirms) != 1 || confirms[0].Title != "Delete task?" || !containsSubstr(confirms[0].Message, "Weekly Audit") {
		t.Fatalf("unexpected delete confirmation: %+v", confirms)
	}
	confirmAnswer(1)
	tester.Frame()
	waitFor(t, func() bool {
		h.mu.Lock()
		defer h.mu.Unlock()
		return len(h.settledReqs) == 1 && h.settledReqs[0].Command.Kind == "remove_scheduled_task"
	}, "remove_scheduled_task dispatched after confirm")

	// Close window bindings releases subscriptions cleanly.
	app.CloseWindowBindings()
	if h.unsubCount.Load() != 1 {
		t.Fatalf("OnChange unsubscribe count = %d, want 1", h.unsubCount.Load())
	}
}

func containsSubstr(s, sub string) bool {
	for i := 0; i+len(sub) <= len(s); i++ {
		if s[i:i+len(sub)] == sub {
			return true
		}
	}
	return false
}

func TestCanonicalNodeLabelAndModelOptionsDelegation(t *testing.T) {
	local := protocol.NodeID{0xaa, 0xbb, 0xcc, 0xdd, 0x11, 0x22, 0x33, 0x44}
	unknown := protocol.NodeID{0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03, 0x04}

	// Fallback without NodeLabel uses 8-char short ID, never raw 64-char hex.
	bare := newController(t.Context(), viewapi.Services{Node: local}, func() bool { return true })
	defer bare.close()
	bare.mu.Lock()
	if got := bare.nodeLabelLocked(unknown); got != "deadbeef" {
		t.Fatalf("short fallback label = %q, want deadbeef", got)
	}
	if len(bare.nodes) != 1 || bare.nodes[0].Label != "aabbccdd" {
		t.Fatalf("fallback node list = %+v", bare.nodes)
	}
	bare.mu.Unlock()

	// Delegates to canonical services.NodeLabel and services.ModelOptions when present.
	wantOpt := kit.ModelOption{ID: "p/m", Name: "Canonical Model", Provider: "OpenAI", Brand: "openai", BrandName: "OpenAI", Score: 9.2}
	svc := viewapi.Services{
		Node: local,
		NodeLabel: func(id protocol.NodeID) string {
			if id == local {
				return "本地"
			}
			return "Remote Box"
		},
		ModelOptions: func(id protocol.NodeID) []kit.ModelOption {
			if id == local {
				return []kit.ModelOption{wantOpt}
			}
			return nil
		},
	}
	ctrl := newController(t.Context(), svc, func() bool { return true })
	defer ctrl.close()
	ctrl.mu.Lock()
	if got := ctrl.nodeLabelLocked(local); got != "本地" {
		t.Fatalf("NodeLabel(local) = %q, want 本地", got)
	}
	if got := ctrl.nodeLabelLocked(unknown); got != "Remote Box" {
		t.Fatalf("NodeLabel(unknown) = %q, want Remote Box", got)
	}
	opts := ctrl.modelOptionsLocked(local)
	if len(opts) != 1 || opts[0].Name != wantOpt.Name || opts[0].BrandName != wantOpt.BrandName || opts[0].Score != wantOpt.Score {
		t.Fatalf("modelOptionsLocked = %+v, want %+v", opts, wantOpt)
	}
	ctrl.mu.Unlock()
}

func TestDirectoryCopiesAndFailedReadsFollowVisibilityChanges(t *testing.T) {
	var visible bool
	var snapshots, loads int
	ctrl := newController(t.Context(), viewapi.Services{
		Node: protocol.NodeID{1},
		Snapshot: func(protocol.NodeID) (*protocol.Snapshot, bool) {
			snapshots++
			return nil, false
		},
		Async: func(work func()) bool { work(); return true },
		PrepareOn: func(protocol.NodeID, protocol.Command) (protocol.Request, error) {
			loads++
			return protocol.Request{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable}
		},
	}, func() bool { return visible })
	defer ctrl.close()
	ctrl.handleChange()
	ctrl.sync()
	if snapshots != 0 || loads != 0 {
		t.Fatalf("hidden page took %d snapshots and started %d reads", snapshots, loads)
	}
	visible = true
	for range 8 {
		ctrl.sync()
	}
	if snapshots != 1 || loads != 1 {
		t.Fatalf("unchanged frames took %d snapshots and started %d reads", snapshots, loads)
	}
	ctrl.handleChange()
	if snapshots != 2 || loads != 2 {
		t.Fatalf("state change did not refresh after failure: snapshots=%d reads=%d", snapshots, loads)
	}
}
