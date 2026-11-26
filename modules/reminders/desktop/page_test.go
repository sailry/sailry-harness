package remindersdesktop

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

type fakeRemindersHarness struct {
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
	revealCount  atomic.Int32
	unsubCount   atomic.Int32
	wakeCount    atomic.Int32
}

func newFakeRemindersHarness() (*fakeRemindersHarness, protocol.ProjectID) {
	localNode := protocol.NodeID{3}
	remoteNode := protocol.NodeID{4}
	projID := protocol.NewProjectID()

	localSnap := protocol.DefaultSnapshot()
	localSnap.Node = localNode
	localSnap.Projects = []protocol.Project{{ID: projID, Name: "Personal", Path: "/home/personal"}}

	remoteSnap := protocol.DefaultSnapshot()
	remoteSnap.Node = remoteNode

	h := &fakeRemindersHarness{
		localNode:  localNode,
		remoteNode: remoteNode,
		nodes: []viewapi.NodeInfo{
			{ID: localNode, Label: "Local Mac", Connected: true, Local: true},
			{ID: remoteNode, Label: "Workstation", Connected: true, Local: false},
		},
		snapshots: map[protocol.NodeID]*protocol.Snapshot{
			localNode:  &localSnap,
			remoteNode: &remoteSnap,
		},
	}
	return h, projID
}

func (h *fakeRemindersHarness) services() viewapi.Services {
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
		RevealDetails: func() { h.revealCount.Add(1) },
		OnChange: func(_ func()) func() {
			var once sync.Once
			return func() { once.Do(func() { h.unsubCount.Add(1) }) }
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
	if p.Page != PageID || p.Feature.ID != "page:reminders" || p.Feature.Icon != icons.BellNav ||
		!p.Feature.DefaultPinned || p.Feature.LegacyPackage != "reminders" || !p.ReplacesLegacyViews {
		t.Fatalf("unexpected NativePage registration: %+v", p)
	}
	cat := i18n.New(assembly.Locales...)
	if got := cat.In("en", p.Feature.Label); got != "Reminders" {
		t.Fatalf("en label = %q", got)
	}
	if got := cat.In("zh-CN", p.Feature.Label); got != "待办提醒" {
		t.Fatalf("zh-CN label = %q", got)
	}
}

func TestControllerCRUDAndPastDueEditRule(t *testing.T) {
	h, projID := newFakeRemindersHarness()
	fixedNow := time.Date(2026, 4, 6, 12, 0, 0, 0, time.UTC)
	rev := uint64(4)
	pastDue := fixedNow.Add(-30 * time.Minute).UnixMilli()
	rem := protocol.Reminder{
		ID:            "55555555-5555-7555-8555-555555555555",
		ExecutionNode: &h.localNode,
		Project:       &projID,
		Title:         "Submit expense report",
		Message:       "Attach hotel invoice",
		Completed:     false,
		DueMs:         &pastDue,
	}

	h.executeFn = func(_ context.Context, _ protocol.NodeID, req protocol.Request) (protocol.Output, error) {
		switch req.Command.Kind {
		case "list_reminders":
			return protocol.Output{Kind: "reminder_page", Data: protocol.ReminderPage{
				Items: []protocol.ReminderEntry{{Reminder: rem, Revision: &rev}},
			}}, nil
		case "read_reminder_history":
			return protocol.Output{Kind: "reminder_history", Data: protocol.ReminderHistory{
				Items: []protocol.DispatchJob{{ID: protocol.NewJobID(), Handler: rem.ID, Status: protocol.DispatchStatusCompleted, CreatedMs: pastDue, FinishedMs: &pastDue}},
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
		return ctrl.loaded && !ctrl.loading && len(ctrl.items) == 1
	}, "initial load")

	// Editing an existing reminder while keeping past due_ms unchanged succeeds.
	itemCopy := ctrl.items[0]
	ctrl.openEditor(&itemCopy)
	ctrl.mu.Lock()
	ctrl.editor.title = "Submit Q1 expense report"
	ctrl.mu.Unlock()
	ctrl.saveEditor()
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy && ctrl.editor == nil
	}, "unchanged past due edit saved")

	h.mu.Lock()
	if len(h.settledReqs) != 1 || h.settledReqs[0].Command.Kind != "save_reminder" {
		t.Fatalf("settled reqs = %+v", h.settledReqs)
	}
	savePayload := h.settledReqs[0].Command.Data.(*protocol.CommandSaveReminderPayload)
	if savePayload.ExpectedValue == nil || savePayload.Revision != rev || savePayload.Reminder.DueMs == nil || *savePayload.Reminder.DueMs != pastDue {
		t.Fatalf("unexpected save payload: %+v", savePayload)
	}
	h.mu.Unlock()

	// Creating a new reminder with a past due_ms is rejected.
	ctrl.openEditor(nil)
	ctrl.mu.Lock()
	ctrl.editor.title = "Invalid past reminder"
	ctrl.editor.timed = true
	pastTime := fixedNow.Add(-10 * time.Minute)
	ctrl.editor.dueDate = pastTime
	ctrl.editor.dueTime = pastTime
	ctrl.mu.Unlock()
	ctrl.saveEditor()
	ctrl.mu.Lock()
	if ctrl.editor == nil || len(ctrl.toasts) == 0 || ctrl.toasts[len(ctrl.toasts)-1].key != "reminders_invalid_time" {
		t.Fatalf("expected invalid time toast and retained editor, got editor=%v toasts=%+v", ctrl.editor, ctrl.toasts)
	}
	ctrl.mu.Unlock()
	ctrl.closeEditor()

	// Toggle completion dispatches save_reminder with Completed=true.
	ctrl.toggleCompleted(ctrl.items[0], true)
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy
	}, "toggle completed")
	h.mu.Lock()
	if len(h.settledReqs) != 2 || !h.settledReqs[1].Command.Data.(*protocol.CommandSaveReminderPayload).Reminder.Completed {
		t.Fatalf("toggleCompleted did not save completed=true: %+v", h.settledReqs)
	}
	h.mu.Unlock()
}

func TestControllerUncertainMutationRecoveryReusesRequestID(t *testing.T) {
	h, _ := newFakeRemindersHarness()
	fixedNow := time.Date(2026, 4, 6, 12, 0, 0, 0, time.UTC)
	rev := uint64(1)
	rem := protocol.Reminder{
		ID:            "66666666-6666-7666-8666-666666666666",
		ExecutionNode: &h.localNode,
		Title:         "Renew certificate",
	}

	h.executeFn = func(_ context.Context, _ protocol.NodeID, req protocol.Request) (protocol.Output, error) {
		if req.Command.Kind == "list_reminders" {
			return protocol.Output{Kind: "reminder_page", Data: protocol.ReminderPage{
				Items: []protocol.ReminderEntry{{Reminder: rem, Revision: &rev}},
			}}, nil
		}
		return protocol.Output{}, nil
	}

	var attempts atomic.Int32
	h.settleFn = func(_ context.Context, _ protocol.NodeID, _ protocol.Request) (protocol.Output, error) {
		if attempts.Add(1) == 1 {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "timeout"}
		}
		return protocol.Output{Kind: "reminder_entry"}, nil
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

	ctrl.toggleCompleted(ctrl.items[0], true)
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy && ctrl.pending != nil
	}, "pending after OutcomeUnknown")

	ctrl.mu.Lock()
	origID := ctrl.pending.request.ID
	ctrl.mu.Unlock()

	ctrl.retryPending()
	waitFor(t, func() bool {
		ctrl.mu.Lock()
		defer ctrl.mu.Unlock()
		return !ctrl.busy && ctrl.pending == nil
	}, "retry succeeded")

	h.mu.Lock()
	if len(h.resolvedReqs) != 1 || h.resolvedReqs[0].ID != origID {
		t.Fatalf("ResolveOn did not check original RequestID: %+v", h.resolvedReqs)
	}
	if len(h.settledReqs) != 2 || h.settledReqs[0].ID != origID || h.settledReqs[1].ID != origID {
		t.Fatalf("SettleOn did not reuse original RequestID: %+v", h.settledReqs)
	}
	h.mu.Unlock()
}

func TestControllerCloseAndStaleReplyDiscard(t *testing.T) {
	h, _ := newFakeRemindersHarness()
	release := make(chan struct{})
	started := make(chan struct{}, 1)
	h.executeFn = func(_ context.Context, _ protocol.NodeID, _ protocol.Request) (protocol.Output, error) {
		select {
		case started <- struct{}{}:
		default:
		}
		<-release
		return protocol.Output{Kind: "reminder_page", Data: protocol.ReminderPage{
			Items: []protocol.ReminderEntry{{Reminder: protocol.Reminder{ID: "late", Title: "Stale"}}},
		}}, nil
	}

	ctrl := newController(t.Context(), h.services(), func() bool { return true })
	ctrl.sync()
	<-started

	ctrl.close()
	ctrl.close()
	if h.unsubCount.Load() != 1 {
		t.Fatalf("OnChange unsubscribe count = %d, want 1", h.unsubCount.Load())
	}
	if ctrl.ctx.Err() == nil {
		t.Fatal("context not cancelled on close")
	}
	close(release)
	time.Sleep(30 * time.Millisecond)

	ctrl.mu.Lock()
	if len(ctrl.items) != 0 {
		t.Fatalf("stale reply mutated closed controller: %+v", ctrl.items)
	}
	ctrl.mu.Unlock()
}

func TestRemindersUIAndDetailsAndDeleteConfirm(t *testing.T) {
	h, projID := newFakeRemindersHarness()
	now := time.Now()
	past := now.Add(-15 * time.Minute).UnixMilli()
	rev := uint64(1)
	openRem := protocol.Reminder{
		ID:            "77777777-7777-7777-8777-777777777777",
		ExecutionNode: &h.localNode,
		Project:       &projID,
		Title:         "Call dentist",
		Message:       "Confirm Tuesday slot",
		Completed:     false,
		DueMs:         &past,
	}
	doneRem := protocol.Reminder{
		ID:            "88888888-8888-7888-8888-888888888888",
		ExecutionNode: &h.localNode,
		Title:         "Buy coffee",
		Completed:     true,
	}

	h.executeFn = func(_ context.Context, _ protocol.NodeID, req protocol.Request) (protocol.Output, error) {
		switch req.Command.Kind {
		case "list_reminders":
			return protocol.Output{Kind: "reminder_page", Data: protocol.ReminderPage{
				Items: []protocol.ReminderEntry{
					{Reminder: openRem, Revision: &rev},
					{Reminder: doneRem, Revision: &rev},
				},
			}}, nil
		case "read_reminder_history":
			return protocol.Output{Kind: "reminder_history", Data: protocol.ReminderHistory{
				Items: []protocol.DispatchJob{{ID: protocol.NewJobID(), Handler: openRem.ID, Status: protocol.DispatchStatusCompleted, CreatedMs: past, FinishedMs: &past}},
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
		return tester.HasText("Call dentist") && tester.HasText("Buy coffee") && tester.HasText("Overdue")
	}, "reminders rendered with overdue badge and completed section")

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

	// Collapse Completed section hides Buy coffee.
	clickEnabledButton("Completed 1")
	if tester.HasText("Buy coffee") {
		t.Fatal("collapsing Completed section did not hide completed reminder")
	}

	// Open Details right column.
	clickEnabledButton("Details")
	waitFor(t, func() bool {
		tester.Frame()
		return tester.HasText("Confirm Tuesday slot")
	}, "reminder details panel rendered")

	// Switch to Records tab and verify delivery record renders.
	clickEnabledButton("Records")
	waitFor(t, func() bool {
		tester.Frame()
		return tester.HasText("Notified")
	}, "reminder delivery timeline rendered")

	// Open New reminder editor dialog and cancel it cleanly.
	clickEnabledButton("New reminder")
	if !tester.HasText("Title") || !tester.HasText("Notes") || !tester.HasText("Remind me") {
		t.Fatalf("reminder editor dialog did not render expected fields: %v", tester.Texts())
	}
	clickEnabledButton("Cancel")

	// Trigger OS-native delete confirmation.
	clickEnabledButton("Delete")
	if len(confirms) != 1 || confirms[0].Title != "Delete reminder?" || !containsSubstr(confirms[0].Message, "Call dentist") {
		t.Fatalf("unexpected delete confirmation: %+v", confirms)
	}
	confirmAnswer(1)
	tester.Frame()
	waitFor(t, func() bool {
		h.mu.Lock()
		defer h.mu.Unlock()
		return len(h.settledReqs) == 1 && h.settledReqs[0].Command.Kind == "remove_reminder"
	}, "remove_reminder dispatched after confirm")

	// Close window bindings releases subscriptions cleanly.
	app.CloseWindowBindings()
	if h.unsubCount.Load() != 1 {
		t.Fatalf("OnChange unsubscribe count = %d, want 1", h.unsubCount.Load())
	}
}

func TestRemindersHidesOpenListWhenOnlyCompletedExist(t *testing.T) {
	h, _ := newFakeRemindersHarness()
	rev := uint64(1)
	h.executeFn = func(_ context.Context, _ protocol.NodeID, req protocol.Request) (protocol.Output, error) {
		if req.Command.Kind == "list_reminders" {
			return protocol.Output{Kind: "reminder_page", Data: protocol.ReminderPage{
				Items: []protocol.ReminderEntry{{
					Reminder: protocol.Reminder{ID: "99999999-9999-7999-8999-999999999999", ExecutionNode: &h.localNode, Title: "Archived task", Completed: true},
					Revision: &rev,
				}},
			}}, nil
		}
		return protocol.Output{}, nil
	}

	assembly := shell.Assemble(Register)
	app := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
	app.BindWindow(viewapi.WithServices(t.Context(), h.services()))
	defer app.CloseWindowBindings()
	app.Navigate(PageID)
	app.SyncNativePages()

	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		app.View(c)
	}, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tester.SetLocale("en")

	waitFor(t, func() bool {
		tester.Frame()
		return tester.HasText("Archived task")
	}, "completed reminder rendered")
	if tester.HasText("No reminders") {
		t.Fatal("open list empty state must be hidden when only completed reminders exist")
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

func TestCanonicalNodeLabelFallbackAndDelegation(t *testing.T) {
	local := protocol.NodeID{0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc}
	unknown := protocol.NodeID{0xfe, 0xed, 0xfa, 0xce, 0x11, 0x22}

	bare := newController(t.Context(), viewapi.Services{Node: local}, func() bool { return true })
	defer bare.close()
	bare.mu.Lock()
	if got := bare.nodeLabelLocked(unknown); got != "feedface" {
		t.Fatalf("short fallback label = %q, want feedface", got)
	}
	if len(bare.nodes) != 1 || bare.nodes[0].Label != "12345678" {
		t.Fatalf("fallback node list = %+v", bare.nodes)
	}
	bare.mu.Unlock()

	ctrl := newController(t.Context(), viewapi.Services{
		Node: local,
		NodeLabel: func(id protocol.NodeID) string {
			if id == local {
				return "Local"
			}
			return "Peer A"
		},
	}, func() bool { return true })
	defer ctrl.close()
	ctrl.mu.Lock()
	if got := ctrl.nodeLabelLocked(local); got != "Local" {
		t.Fatalf("NodeLabel(local) = %q, want Local", got)
	}
	if got := ctrl.nodeLabelLocked(unknown); got != "Peer A" {
		t.Fatalf("NodeLabel(unknown) = %q, want Peer A", got)
	}
	ctrl.mu.Unlock()
}
