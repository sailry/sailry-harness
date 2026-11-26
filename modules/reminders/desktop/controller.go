package remindersdesktop

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const tickInterval = 30 * time.Second

type projectOption struct {
	Node    protocol.NodeID
	Project protocol.Project
}

type panelState struct {
	reminderID string
	tab        string // "details" or "records"
}

type pendingMutation struct {
	target               protocol.NodeID
	request              protocol.Request
	kind                 string // "save", "remove"
	errorKey             string
	closeEditorOnSuccess bool
}

type editorState struct {
	editing       bool
	original      *protocol.Reminder
	revision      uint64
	reminderID    string
	executionNode protocol.NodeID
	hasExecNode   bool
	title         string
	message       string
	completed     bool
	project       *protocol.ProjectID
	notifiedMs    *int64

	timed       bool
	dueDate     time.Time
	dueTime     time.Time
	originalDue *int64
}

type toastNotice struct {
	outcome kit.Outcome
	key     string
}

type controller struct {
	mu       sync.Mutex
	ctx      context.Context
	cancel   context.CancelFunc
	services viewapi.Services
	visible  func() bool
	nowFn    func() time.Time
	loc      *time.Location

	closed        bool
	onChangeUnsub func()
	tickTimer     *time.Timer
	needReload    bool
	wasVisible    bool

	reloadGen   uint64
	mutationGen uint64
	loading     bool
	loaded      bool
	dirty       bool

	nodes         []viewapi.NodeInfo
	projects      []projectOption
	items         []ReminderItem
	projFilter    string // "all", "none", or ProjectID string
	showCompleted bool

	panel         *panelState
	history       []protocol.DispatchJob
	historyID     string
	historyLoaded bool

	busy     bool
	pending  *pendingMutation
	editor   *editorState
	deleting *ReminderItem
	toasts   []toastNotice
}

func newController(parent context.Context, services viewapi.Services, visible func() bool) *controller {
	if parent == nil {
		parent = context.Background()
	}
	ctx, cancel := context.WithCancel(parent)
	c := &controller{
		ctx:           ctx,
		cancel:        cancel,
		services:      services,
		visible:       visible,
		nowFn:         time.Now,
		loc:           time.Local,
		projFilter:    "all",
		showCompleted: true,
	}
	c.refreshDirectoryLocked()
	if services.OnChange != nil {
		c.onChangeUnsub = services.OnChange(c.handleChange)
	}
	return c
}

func (ctrl *controller) now() time.Time {
	if ctrl.nowFn != nil {
		return ctrl.nowFn()
	}
	return time.Now()
}

func (ctrl *controller) isVisible() bool {
	if ctrl.visible == nil {
		return true
	}
	return ctrl.visible()
}

func (ctrl *controller) close() {
	ctrl.mu.Lock()
	if ctrl.closed {
		ctrl.mu.Unlock()
		return
	}
	ctrl.closed = true
	ctrl.reloadGen++
	ctrl.mutationGen++
	cancel := ctrl.cancel
	unsub := ctrl.onChangeUnsub
	ctrl.onChangeUnsub = nil
	timer := ctrl.tickTimer
	ctrl.tickTimer = nil
	ctrl.mu.Unlock()

	if timer != nil {
		timer.Stop()
	}
	if unsub != nil {
		unsub()
	}
	if cancel != nil {
		cancel()
	}
}

func (ctrl *controller) sync() {
	ctrl.mu.Lock()
	if ctrl.closed {
		ctrl.mu.Unlock()
		return
	}
	ctrl.refreshDirectoryLocked()
	vis := ctrl.isVisible()
	shouldLoad := vis && (!ctrl.loaded && !ctrl.loading || ctrl.needReload || !ctrl.wasVisible && !ctrl.loading)
	ctrl.wasVisible = vis
	if shouldLoad {
		ctrl.needReload = false
	}
	ctrl.updateTickerLocked(vis)
	ctrl.mu.Unlock()

	if shouldLoad {
		ctrl.reload()
	}
}

func (ctrl *controller) handleChange() {
	ctrl.mu.Lock()
	if ctrl.closed {
		ctrl.mu.Unlock()
		return
	}
	ctrl.refreshDirectoryLocked()
	if !ctrl.wasVisible {
		ctrl.needReload = true
		ctrl.mu.Unlock()
		return
	}
	ctrl.mu.Unlock()
	ctrl.reload()
}

func (ctrl *controller) updateTickerLocked(visible bool) {
	needsTick := visible && !ctrl.closed && ctrl.hasTimedOpenLocked()
	if !needsTick {
		if ctrl.tickTimer != nil {
			ctrl.tickTimer.Stop()
			ctrl.tickTimer = nil
		}
		return
	}
	if ctrl.tickTimer == nil {
		ctrl.tickTimer = time.AfterFunc(tickInterval, ctrl.onTick)
	}
}

func (ctrl *controller) hasTimedOpenLocked() bool {
	for _, item := range ctrl.items {
		if !item.Reminder.Completed && item.Reminder.DueMs != nil {
			return true
		}
	}
	return false
}

func (ctrl *controller) onTick() {
	ctrl.mu.Lock()
	if ctrl.closed {
		ctrl.mu.Unlock()
		return
	}
	ctrl.tickTimer = nil
	vis := ctrl.wasVisible
	ctrl.updateTickerLocked(vis)
	wake := ctrl.services.Wake
	ctrl.mu.Unlock()
	if vis && wake != nil {
		wake()
	}
}

func (ctrl *controller) wake() {
	if ctrl.services.Wake != nil {
		ctrl.services.Wake()
	}
}

func (ctrl *controller) runAsync(work func()) {
	if ctrl.services.Async != nil {
		if ctrl.services.Async(work) {
			return
		}
		return
	}
	go work()
}

func (ctrl *controller) refreshDirectoryLocked() {
	var nodes []viewapi.NodeInfo
	if ctrl.services.Nodes != nil {
		nodes = ctrl.services.Nodes()
	}
	if len(nodes) == 0 && ctrl.services.Node != (protocol.NodeID{}) {
		connected := ctrl.services.Connected == nil || ctrl.services.Connected()
		nodes = []viewapi.NodeInfo{{ID: ctrl.services.Node, Label: ctrl.nodeLabelLocked(ctrl.services.Node), Connected: connected, Local: true}}
	}
	ctrl.nodes = nodes

	var projects []projectOption
	if ctrl.services.Snapshot != nil {
		for _, node := range nodes {
			snap, ok := ctrl.services.Snapshot(node.ID)
			if !ok || snap == nil {
				continue
			}
			for _, pr := range snap.Projects {
				projects = append(projects, projectOption{Node: node.ID, Project: pr})
			}
		}
	}
	ctrl.projects = projects
}

func (ctrl *controller) reload() {
	ctrl.mu.Lock()
	if ctrl.closed {
		ctrl.mu.Unlock()
		return
	}
	if ctrl.loading {
		ctrl.dirty = true
		ctrl.mu.Unlock()
		return
	}
	ctrl.loading = true
	ctrl.dirty = false
	ctrl.reloadGen++
	gen := ctrl.reloadGen
	ctx := ctrl.ctx
	localNode := ctrl.services.Node
	ctrl.mu.Unlock()
	ctrl.wake()

	ctrl.runAsync(func() {
		for {
			ctrl.mu.Lock()
			if ctrl.closed || ctrl.reloadGen != gen {
				ctrl.mu.Unlock()
				return
			}
			ctrl.dirty = false
			panelID := ""
			if ctrl.panel != nil {
				panelID = ctrl.panel.reminderID
			}
			ctrl.mu.Unlock()

			entries, listErr := ctrl.fetchAllReminders(ctx, localNode)
			if listErr != nil {
				ctrl.mu.Lock()
				stale := ctrl.closed || ctrl.reloadGen != gen
				if !stale && ctx.Err() == nil {
					ctrl.pushToastLocked(kit.Failed, "reminders_load_failed")
				}
				again := !stale && ctrl.dirty
				if !again && !stale {
					ctrl.loading = false
				}
				ctrl.mu.Unlock()
				ctrl.wake()
				if again {
					continue
				}
				return
			}

			items := make([]ReminderItem, 0, len(entries))
			var selected *ReminderItem
			for _, entry := range entries {
				it := ReminderItem{
					Reminder:    entry.Reminder,
					Revision:    entry.Revision,
					Unavailable: entry.Unavailable,
				}
				items = append(items, it)
				if panelID != "" && entry.Reminder.ID == panelID {
					cp := it
					selected = &cp
				}
			}

			var jobs []protocol.DispatchJob
			var historyErr error
			if selected != nil {
				target := ctrl.executionNodeOf(selected.Reminder)
				jobs, historyErr = ctrl.fetchHistory(ctx, target, selected.Reminder.ID)
			}

			ctrl.mu.Lock()
			if ctrl.closed || ctrl.reloadGen != gen {
				ctrl.mu.Unlock()
				return
			}
			ctrl.refreshDirectoryLocked()
			ctrl.items = items
			ctrl.loaded = true
			if ctrl.panel != nil && !slices.ContainsFunc(items, func(it ReminderItem) bool { return it.Reminder.ID == ctrl.panel.reminderID }) {
				ctrl.panel = nil
			}
			currentPanelID := ""
			if ctrl.panel != nil {
				currentPanelID = ctrl.panel.reminderID
			}
			if panelID != "" && panelID == currentPanelID {
				if historyErr != nil {
					ctrl.history = nil
					ctrl.historyID = panelID
					ctrl.historyLoaded = false
					if ctx.Err() == nil {
						ctrl.pushToastLocked(kit.Failed, "reminders_execution_unavailable")
					}
				} else {
					ctrl.history = jobs
					ctrl.historyID = panelID
					ctrl.historyLoaded = true
				}
			}
			ctrl.updateTickerLocked(ctrl.wasVisible)
			again := ctrl.dirty
			if !again {
				ctrl.loading = false
			}
			ctrl.mu.Unlock()
			ctrl.wake()
			if !again {
				return
			}
		}
	})
}

func (ctrl *controller) executionNodeOf(rem protocol.Reminder) protocol.NodeID {
	if rem.ExecutionNode != nil && *rem.ExecutionNode != (protocol.NodeID{}) {
		return *rem.ExecutionNode
	}
	return ctrl.services.Node
}

func (ctrl *controller) prepareCommand(node protocol.NodeID, cmd protocol.Command) (protocol.Request, error) {
	if ctrl.services.PrepareOn != nil {
		return ctrl.services.PrepareOn(node, cmd)
	}
	if ctrl.services.Prepare != nil {
		req := ctrl.services.Prepare(cmd)
		if req.ID == (protocol.RequestID{}) {
			return protocol.Request{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "request preparation unavailable"}
		}
		return req, nil
	}
	return protocol.Request{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "request preparation unavailable"}
}

func (ctrl *controller) executeCommand(ctx context.Context, node protocol.NodeID, cmd protocol.Command) (protocol.Output, error) {
	req, err := ctrl.prepareCommand(node, cmd)
	if err != nil {
		return protocol.Output{}, err
	}
	if ctrl.services.ExecuteOn != nil {
		return ctrl.services.ExecuteOn(ctx, node, req)
	}
	if ctrl.services.Execute != nil {
		return ctrl.services.Execute(ctx, req)
	}
	return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "execution service unavailable"}
}

func (ctrl *controller) settleRequest(ctx context.Context, node protocol.NodeID, req protocol.Request) (protocol.Output, error) {
	if ctrl.services.SettleOn != nil {
		return ctrl.services.SettleOn(ctx, node, req)
	}
	if ctrl.services.Settle != nil {
		return ctrl.services.Settle(ctx, req)
	}
	if ctrl.services.ExecuteOn != nil {
		return ctrl.services.ExecuteOn(ctx, node, req)
	}
	if ctrl.services.Execute != nil {
		return ctrl.services.Execute(ctx, req)
	}
	return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "settlement service unavailable"}
}

func (ctrl *controller) fetchAllReminders(ctx context.Context, node protocol.NodeID) ([]protocol.ReminderEntry, error) {
	var all []protocol.ReminderEntry
	var after *string
	for {
		out, err := ctrl.executeCommand(ctx, node, protocol.Command{
			Kind: "list_reminders",
			Data: &protocol.CommandListRemindersPayload{After: after},
		})
		if err != nil {
			return nil, err
		}
		page, ok := decodeOutput[protocol.ReminderPage](out)
		if !ok {
			return nil, fmt.Errorf("unexpected list_reminders output %q", out.Kind)
		}
		all = append(all, page.Items...)
		if page.After == nil || *page.After == "" {
			break
		}
		next := *page.After
		after = &next
	}
	return all, nil
}

func (ctrl *controller) fetchHistory(ctx context.Context, node protocol.NodeID, reminderID string) ([]protocol.DispatchJob, error) {
	out, err := ctrl.executeCommand(ctx, node, protocol.Command{
		Kind: "read_reminder_history",
		Data: &protocol.CommandReadReminderHistoryPayload{Handler: reminderID},
	})
	if err != nil {
		return nil, err
	}
	hist, ok := decodeOutput[protocol.ReminderHistory](out)
	if !ok {
		return nil, fmt.Errorf("unexpected read_reminder_history output %q", out.Kind)
	}
	return hist.Items, nil
}

func (ctrl *controller) inspect(item ReminderItem, tab string) {
	ctrl.mu.Lock()
	needReload := false
	if ctrl.panel == nil || ctrl.panel.reminderID != item.Reminder.ID {
		ctrl.panel = &panelState{reminderID: item.Reminder.ID, tab: tab}
		ctrl.history = nil
		ctrl.historyID = ""
		ctrl.historyLoaded = false
		needReload = true
	} else {
		ctrl.panel.tab = tab
	}
	reveal := ctrl.services.RevealDetails
	ctrl.mu.Unlock()
	if reveal != nil {
		reveal()
	}
	ctrl.wake()
	if needReload {
		ctrl.reload()
	}
}

func (ctrl *controller) closePanel() {
	ctrl.mu.Lock()
	ctrl.panel = nil
	ctrl.mu.Unlock()
	ctrl.wake()
}

func (ctrl *controller) setPanelTab(tab string) {
	ctrl.mu.Lock()
	if ctrl.panel != nil {
		ctrl.panel.tab = tab
	}
	ctrl.mu.Unlock()
	ctrl.wake()
}

func (ctrl *controller) openEditor(item *ReminderItem) {
	ctrl.mu.Lock()
	if ctrl.busy || ctrl.pending != nil {
		ctrl.mu.Unlock()
		return
	}
	ctrl.refreshDirectoryLocked()
	ctrl.deleting = nil
	defaultDue := ctrl.now().In(ctrl.loc).Add(5 * time.Minute)
	var ed *editorState
	if item == nil {
		ed = &editorState{
			executionNode: ctrl.services.Node,
			hasExecNode:   true,
			dueDate:       defaultDue,
			dueTime:       defaultDue,
		}
	} else {
		orig := canonicalReminderSnapshot(item.Reminder)
		var rev uint64
		if item.Revision != nil {
			rev = *item.Revision
		}
		execNode := ctrl.executionNodeOf(item.Reminder)
		ed = &editorState{
			editing:       true,
			original:      &orig,
			revision:      rev,
			reminderID:    item.Reminder.ID,
			executionNode: execNode,
			hasExecNode:   item.Reminder.ExecutionNode != nil && *item.Reminder.ExecutionNode != (protocol.NodeID{}) || ctrl.services.Node != (protocol.NodeID{}),
			title:         item.Reminder.Title,
			message:       item.Reminder.Message,
			completed:     item.Reminder.Completed,
			project:       cloneProjectID(item.Reminder.Project),
			notifiedMs:    cloneInt64Ptr(item.Reminder.NotifiedMs),
			timed:         item.Reminder.DueMs != nil,
			dueDate:       defaultDue,
			dueTime:       defaultDue,
			originalDue:   cloneInt64Ptr(item.Reminder.DueMs),
		}
		if item.Reminder.DueMs != nil {
			dt := time.UnixMilli(*item.Reminder.DueMs).In(ctrl.loc)
			ed.dueDate = dt
			ed.dueTime = dt
		}
	}
	ctrl.editor = ed
	ctrl.mu.Unlock()
	ctrl.wake()
}

func (ctrl *controller) closeEditor() {
	ctrl.mu.Lock()
	if ctrl.busy {
		ctrl.mu.Unlock()
		return
	}
	hadPending := ctrl.pending != nil
	ctrl.pending = nil
	ctrl.editor = nil
	ctrl.deleting = nil
	ctrl.mu.Unlock()
	ctrl.wake()
	if hadPending {
		ctrl.reload()
	}
}

func (ctrl *controller) toggleCompleted(item ReminderItem, completed bool) {
	ctrl.mu.Lock()
	if ctrl.busy || ctrl.pending != nil || item.Unavailable {
		ctrl.mu.Unlock()
		return
	}
	var rev uint64
	if item.Revision != nil {
		rev = *item.Revision
	}
	expected := canonicalReminderSnapshot(item.Reminder)
	updated := expected
	updated.Completed = completed
	target := ctrl.executionNodeOf(item.Reminder)
	cmd := protocol.Command{
		Kind: "save_reminder",
		Data: &protocol.CommandSaveReminderPayload{
			Reminder:      updated,
			Revision:      rev,
			ExpectedValue: &expected,
		},
	}
	ctrl.mu.Unlock()
	ctrl.startMutation(target, cmd, "save", false)
}

func (ctrl *controller) requestDelete(item ReminderItem) {
	ctrl.mu.Lock()
	if ctrl.busy || ctrl.pending != nil || item.Unavailable {
		ctrl.mu.Unlock()
		return
	}
	cp := item
	ctrl.deleting = &cp
	ctrl.mu.Unlock()
	ctrl.wake()
}

func (ctrl *controller) confirmDelete() {
	ctrl.mu.Lock()
	target := ctrl.deleting
	ctrl.deleting = nil
	if target == nil || ctrl.busy || ctrl.pending != nil {
		ctrl.mu.Unlock()
		return
	}
	var rev uint64
	if target.Revision != nil {
		rev = *target.Revision
	}
	expected := canonicalReminderSnapshot(target.Reminder)
	nodeID := ctrl.executionNodeOf(target.Reminder)
	cmd := protocol.Command{
		Kind: "remove_reminder",
		Data: &protocol.CommandRemoveReminderPayload{
			ID:            target.Reminder.ID,
			Revision:      rev,
			ExpectedValue: &expected,
		},
	}
	ctrl.mu.Unlock()
	ctrl.startMutation(nodeID, cmd, "remove", false)
}

func (ctrl *controller) saveEditor() {
	ctrl.mu.Lock()
	if ctrl.pending != nil {
		ctrl.mu.Unlock()
		ctrl.retryPending()
		return
	}
	ed := ctrl.editor
	if ed == nil || ctrl.busy {
		ctrl.mu.Unlock()
		return
	}
	rem, errKey := ctrl.buildReminderFromEditorLocked(ed)
	if errKey != "" {
		ctrl.pushToastLocked(kit.Failed, errKey)
		ctrl.mu.Unlock()
		ctrl.wake()
		return
	}
	payload := &protocol.CommandSaveReminderPayload{
		Reminder: rem,
		Revision: ed.revision,
	}
	if ed.original != nil {
		origCopy := *ed.original
		payload.ExpectedValue = &origCopy
	}
	target := ed.executionNode
	ctrl.mu.Unlock()
	ctrl.startMutation(target, protocol.Command{Kind: "save_reminder", Data: payload}, "save", true)
}

func (ctrl *controller) buildReminderFromEditorLocked(ed *editorState) (protocol.Reminder, string) {
	title := strings.TrimSpace(ed.title)
	if title == "" {
		return protocol.Reminder{}, "reminders_required"
	}
	var due *int64
	if ed.timed {
		var origMs int64
		if ed.originalDue != nil {
			origMs = *ed.originalDue
		}
		atMs, ok := CombineLocalDateTime(ed.dueDate, ed.dueTime, origMs, ctrl.loc)
		if !ok || !ValidateDueMs(atMs, ed.originalDue, ctrl.now().UnixMilli()) {
			return protocol.Reminder{}, "reminders_invalid_time"
		}
		due = &atMs
	}
	var execNode *protocol.NodeID
	if ed.hasExecNode && ed.executionNode != (protocol.NodeID{}) {
		cp := ed.executionNode
		execNode = &cp
	}
	return protocol.Reminder{
		ID:            ed.reminderID,
		ExecutionNode: execNode,
		Project:       cloneProjectID(ed.project),
		Title:         title,
		Message:       ed.message,
		Completed:     ed.completed,
		DueMs:         due,
		NotifiedMs:    cloneInt64Ptr(ed.notifiedMs),
	}, ""
}

func (ctrl *controller) startMutation(target protocol.NodeID, cmd protocol.Command, kind string, closeEditor bool) {
	ctrl.mu.Lock()
	if ctrl.closed || ctrl.busy || ctrl.pending != nil {
		ctrl.mu.Unlock()
		return
	}
	if target == (protocol.NodeID{}) {
		ctrl.pushToastLocked(kit.Failed, "reminders_execution_unknown")
		ctrl.mu.Unlock()
		ctrl.wake()
		return
	}
	if !ctrl.nodeConnectedLocked(target) {
		ctrl.pushToastLocked(kit.Failed, "reminders_execution_unavailable")
		ctrl.mu.Unlock()
		ctrl.wake()
		return
	}
	req, err := ctrl.prepareCommand(target, cmd)
	if err != nil {
		ctrl.pushToastLocked(kit.Failed, "reminders_execution_unavailable")
		ctrl.mu.Unlock()
		ctrl.wake()
		return
	}
	pending := &pendingMutation{
		target:               target,
		request:              req,
		kind:                 kind,
		closeEditorOnSuccess: closeEditor,
	}
	ctrl.pending = pending
	ctrl.busy = true
	ctrl.mutationGen++
	gen := ctrl.mutationGen
	ctx := ctrl.ctx
	ctrl.mu.Unlock()
	ctrl.wake()

	ctrl.runAsync(func() {
		_, settleErr := ctrl.settleRequest(ctx, target, req)
		ctrl.finishMutation(gen, pending, settleErr)
	})
}

func (ctrl *controller) retryPending() {
	ctrl.mu.Lock()
	if ctrl.closed || ctrl.busy || ctrl.pending == nil {
		ctrl.mu.Unlock()
		return
	}
	pending := ctrl.pending
	if !ctrl.nodeConnectedLocked(pending.target) {
		ctrl.pushToastLocked(kit.Failed, "reminders_execution_unavailable")
		ctrl.mu.Unlock()
		ctrl.wake()
		return
	}
	ctrl.busy = true
	ctrl.mutationGen++
	gen := ctrl.mutationGen
	ctx := ctrl.ctx
	ctrl.mu.Unlock()
	ctrl.wake()

	ctrl.runAsync(func() {
		err := ctrl.resolveOrReplay(ctx, pending.target, pending.request)
		ctrl.finishMutation(gen, pending, err)
	})
}

func (ctrl *controller) resolveOrReplay(ctx context.Context, target protocol.NodeID, req protocol.Request) error {
	if ctrl.services.ResolveOn != nil {
		receipt, err := ctrl.services.ResolveOn(ctx, target, req)
		if err != nil {
			return err
		}
		switch receipt.State {
		case "applied":
			return nil
		case "failed":
			if receipt.Fault != nil {
				return receipt.Fault
			}
			return &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "request failed"}
		case "pending", "unknown":
			return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "request outcome is still unknown"}
		case "absent", "replayable":
			_, err := ctrl.settleRequest(ctx, target, req)
			return err
		}
	}
	if ctrl.services.OutcomeOn != nil {
		outcome, err := ctrl.services.OutcomeOn(ctx, target, req)
		if err != nil {
			return err
		}
		switch outcome.Kind {
		case "completed":
			if res, ok := decodeOutcomeResult(outcome.Data); ok {
				if res.Err != nil {
					return res.Err
				}
				return nil
			}
			return nil
		case "admitted", "unknown":
			return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "request outcome is still unknown"}
		case "not_admitted", "unrecorded", "replayable":
			_, err := ctrl.settleRequest(ctx, target, req)
			return err
		}
	}
	_, err := ctrl.settleRequest(ctx, target, req)
	return err
}

func (ctrl *controller) finishMutation(gen uint64, pending *pendingMutation, err error) {
	ctrl.mu.Lock()
	if ctrl.closed || ctrl.mutationGen != gen {
		ctrl.mu.Unlock()
		return
	}
	ctrl.busy = false
	if err == nil {
		ctrl.pending = nil
		if pending.closeEditorOnSuccess {
			ctrl.editor = nil
		}
		ctrl.mu.Unlock()
		ctrl.wake()
		ctrl.reload()
		return
	}

	code := kit.FaultCode(err)
	if isUncertainError(err, code) {
		key := "reminders_unknown"
		if code == protocol.ErrorCodeUnavailable {
			key = "reminders_execution_unavailable"
		}
		updated := *pending
		updated.errorKey = key
		ctrl.pending = &updated
		ctrl.pushToastLocked(kit.Uncertain, key)
		ctrl.mu.Unlock()
		ctrl.wake()
		ctrl.reload()
		return
	}

	ctrl.pending = nil
	outcome := kit.Failed
	key := "reminders_failed"
	switch code {
	case protocol.ErrorCodeRevisionConflict:
		outcome = kit.Conflict
		key = "reminders_conflict"
	case protocol.ErrorCodeNotFound:
		key = "reminders_project_unavailable"
	}
	ctrl.pushToastLocked(outcome, key)
	ctrl.mu.Unlock()
	ctrl.wake()
	ctrl.reload()
}

func (ctrl *controller) dismissPending() {
	ctrl.mu.Lock()
	if ctrl.busy || ctrl.pending == nil {
		ctrl.mu.Unlock()
		return
	}
	ctrl.pending = nil
	ctrl.mu.Unlock()
	ctrl.wake()
	ctrl.reload()
}

func shortNodeID(id protocol.NodeID) string {
	raw := id.String()
	if len(raw) > 8 {
		return raw[:8]
	}
	return raw
}

func (ctrl *controller) nodeLabelLocked(target protocol.NodeID) string {
	if ctrl.services.NodeLabel != nil {
		if label := ctrl.services.NodeLabel(target); label != "" {
			return label
		}
	}
	for _, n := range ctrl.nodes {
		if n.ID == target {
			if n.Label != "" {
				return n.Label
			}
			break
		}
	}
	return shortNodeID(target)
}

func (ctrl *controller) nodeConnectedLocked(target protocol.NodeID) bool {
	for _, n := range ctrl.nodes {
		if n.ID == target {
			return n.Connected
		}
	}
	if target == ctrl.services.Node && ctrl.services.Connected != nil {
		return ctrl.services.Connected()
	}
	return false
}

func (ctrl *controller) projectNameLocked(id *protocol.ProjectID, t Translate) string {
	if id == nil {
		return t("reminders_no_project")
	}
	for _, pr := range ctrl.projects {
		if pr.Project.ID == *id {
			return pr.Project.Name
		}
	}
	return t("reminders_unavailable_project")
}

func (ctrl *controller) pushToastLocked(outcome kit.Outcome, key string) {
	ctrl.toasts = append(ctrl.toasts, toastNotice{outcome: outcome, key: key})
}

func (ctrl *controller) flushToasts(c *ui.Context) {
	ctrl.mu.Lock()
	pending := ctrl.toasts
	ctrl.toasts = nil
	ctrl.mu.Unlock()
	for _, n := range pending {
		kit.Toast(c, n.outcome, i18n.T(c, n.key))
	}
}

func (ctrl *controller) slots() shell.WorkspaceSlots {
	ctrl.mu.Lock()
	hasPanel := ctrl.panel != nil && slices.ContainsFunc(ctrl.items, func(it ReminderItem) bool { return it.Reminder.ID == ctrl.panel.reminderID })
	ctrl.mu.Unlock()
	if !hasPanel {
		return shell.WorkspaceSlots{
			PreferenceKey:       string(PageID),
			DefaultDetailsWidth: detailDefaultWidth,
			MinDetailsWidth:     detailMinWidth,
			DetailsLabel:        "reminders_details",
		}
	}
	return shell.WorkspaceSlots{
		PreferenceKey:       string(PageID),
		DefaultDetailsWidth: detailDefaultWidth,
		MinDetailsWidth:     detailMinWidth,
		DetailsLabel:        "reminders_details",
		DetailsHeader:       ctrl.renderDetailsHeader,
		Details:             ctrl.renderDetailsBody,
	}
}

func decodeOutput[T any](out protocol.Output) (T, bool) {
	var zero T
	switch v := out.Data.(type) {
	case T:
		return v, true
	case *T:
		if v != nil {
			return *v, true
		}
	}
	return zero, false
}

func decodeOutcomeResult(data any) (protocol.Result[protocol.Output, protocol.Fault], bool) {
	switch v := data.(type) {
	case protocol.Result[protocol.Output, protocol.Fault]:
		return v, true
	case *protocol.Result[protocol.Output, protocol.Fault]:
		if v != nil {
			return *v, true
		}
	}
	return protocol.Result[protocol.Output, protocol.Fault]{}, false
}

func isUncertainError(err error, code protocol.ErrorCode) bool {
	if errors.Is(err, context.Canceled) {
		return true
	}
	switch code {
	case "", protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy, protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeInternal, protocol.ErrorCodeCancelled:
		return true
	}
	return false
}

func canonicalReminderSnapshot(rem protocol.Reminder) protocol.Reminder {
	cp := rem
	if rem.ExecutionNode != nil {
		n := *rem.ExecutionNode
		cp.ExecutionNode = &n
	}
	cp.Project = cloneProjectID(rem.Project)
	cp.DueMs = cloneInt64Ptr(rem.DueMs)
	cp.NotifiedMs = cloneInt64Ptr(rem.NotifiedMs)
	return cp
}

func cloneProjectID(id *protocol.ProjectID) *protocol.ProjectID {
	if id == nil {
		return nil
	}
	cp := *id
	return &cp
}

func cloneInt64Ptr(v *int64) *int64 {
	if v == nil {
		return nil
	}
	cp := *v
	return &cp
}
