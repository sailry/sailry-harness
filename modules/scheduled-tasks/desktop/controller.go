package scheduledtasksdesktop

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	historyPageLimit = 32
	recentStripLimit = 14
	tickInterval     = 30 * time.Second
)

type projectOption struct {
	Node    protocol.NodeID
	Project protocol.Project
}

type worktreeOption struct {
	Node     protocol.NodeID
	Worktree protocol.Worktree
}

type panelState struct {
	taskID string
	tab    string // "details" or "records"
}

type pendingMutation struct {
	target               protocol.NodeID
	request              protocol.Request
	kind                 string // "save", "remove", "run", "cancel", "stop"
	errorKey             string
	closeEditorOnSuccess bool
}

type editorState struct {
	editing       bool
	original      *protocol.ScheduledTask
	revision      uint64
	taskID        string
	executionNode protocol.NodeID
	name          string
	prompt        string
	enabled       bool
	queue         string
	project       *protocol.ProjectID
	worktree      *protocol.WorktreeID
	execution     protocol.ConversationExecution
	team          *protocol.TeamSelection
	models        map[protocol.RoleID]protocol.RoleModel
	config        *protocol.SessionConfig
	modelID       string
	effort        string

	timingKind  string // "once", "every", "weekly"
	startDate   time.Time
	startTime   time.Time
	originalAt  int64
	intervalSec string
	weekdays    []uint8
	wallTime    string
	timezone    string
}

type toastNotice struct {
	outcome kit.Outcome
	key     string
	copied  bool
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

	nodes      []viewapi.NodeInfo
	projects   []projectOption
	worktrees  []worktreeOption
	teamsBy    map[protocol.NodeID][]protocol.TeamProfile
	providers  map[protocol.NodeID][]protocol.ConversationProvider
	items      []TaskItem
	projFilter string // "all", "none", or ProjectID string

	panel         *panelState
	history       []protocol.ScheduledTaskHistoryEntry
	historyTaskID string
	historyLoaded bool
	historyPages  int
	historyMore   bool

	busy     bool
	pending  *pendingMutation
	editor   *editorState
	deleting *TaskItem
	toasts   []toastNotice
}

func newController(parent context.Context, services viewapi.Services, visible func() bool) *controller {
	if parent == nil {
		parent = context.Background()
	}
	ctx, cancel := context.WithCancel(parent)
	c := &controller{
		ctx:          ctx,
		cancel:       cancel,
		services:     services,
		visible:      visible,
		nowFn:        time.Now,
		loc:          time.Local,
		projFilter:   "all",
		historyPages: 1,
		teamsBy:      map[protocol.NodeID][]protocol.TeamProfile{},
		providers:    map[protocol.NodeID][]protocol.ConversationProvider{},
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
	if ctrl.editor != nil {
		ctrl.syncEditorLeadModelLocked(ctrl.editor)
	}
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
	needsTick := visible && !ctrl.closed && ctrl.hasLiveClockLocked()
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

func (ctrl *controller) hasLiveClockLocked() bool {
	for _, item := range ctrl.items {
		if item.Status == protocol.DispatchStatusRunning || (item.Task.Enabled && item.NextMs != nil) {
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
	var worktrees []worktreeOption
	teamsBy := make(map[protocol.NodeID][]protocol.TeamProfile, len(nodes))
	providers := make(map[protocol.NodeID][]protocol.ConversationProvider, len(nodes))
	if ctrl.services.Snapshot != nil {
		for _, node := range nodes {
			snap, ok := ctrl.services.Snapshot(node.ID)
			if !ok || snap == nil {
				continue
			}
			for _, pr := range snap.Projects {
				projects = append(projects, projectOption{Node: node.ID, Project: pr})
			}
			for _, wt := range snap.Worktrees {
				worktrees = append(worktrees, worktreeOption{Node: node.ID, Worktree: wt})
			}
			teamsBy[node.ID] = slices.Clone(snap.Teams)
			providers[node.ID] = slices.Clone(snap.Providers)
		}
	}
	ctrl.projects = projects
	ctrl.worktrees = worktrees
	ctrl.teamsBy = teamsBy
	ctrl.providers = providers
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
			var panelTaskID string
			var panelExecNode protocol.NodeID
			pages := ctrl.historyPages
			if pages < 1 {
				pages = 1
			}
			if ctrl.panel != nil {
				panelTaskID = ctrl.panel.taskID
				for _, it := range ctrl.items {
					if it.Task.ID == panelTaskID {
						panelExecNode = ctrl.executionNodeOf(it.Task)
						break
					}
				}
			}
			ctrl.mu.Unlock()

			entries, listErr := ctrl.fetchAllTasks(ctx, localNode)
			if listErr != nil {
				ctrl.mu.Lock()
				stale := ctrl.closed || ctrl.reloadGen != gen
				if !stale && ctx.Err() == nil {
					ctrl.pushToastLocked(kit.Failed, "scheduled_tasks_load_failed")
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

			localRuntime, _ := ctrl.fetchRuntime(ctx, localNode)

			remoteNodes := map[protocol.NodeID]struct{}{}
			for _, entry := range entries {
				execNode := ctrl.executionNodeOf(entry.Task)
				if execNode != localNode {
					remoteNodes[execNode] = struct{}{}
				}
				if panelTaskID != "" && entry.Task.ID == panelTaskID {
					panelExecNode = execNode
				}
			}

			remoteRuntimes := make(map[protocol.NodeID]*protocol.ScheduledTaskRuntime, len(remoteNodes))
			if len(remoteNodes) > 0 {
				var wg sync.WaitGroup
				var rmu sync.Mutex
				for target := range remoteNodes {
					wg.Add(1)
					go func(nodeID protocol.NodeID) {
						defer wg.Done()
						rt, err := ctrl.fetchRuntime(ctx, nodeID)
						rmu.Lock()
						if err == nil {
							remoteRuntimes[nodeID] = &rt
						} else {
							remoteRuntimes[nodeID] = nil
						}
						rmu.Unlock()
					}(target)
				}
				wg.Wait()
			}

			items := make([]TaskItem, 0, len(entries))
			for _, entry := range entries {
				execNode := ctrl.executionNodeOf(entry.Task)
				item := TaskItem{
					Task:        entry.Task,
					Revision:    entry.Revision,
					NextMs:      entry.NextMs,
					Unavailable: entry.Unavailable,
				}
				if execNode == localNode {
					if st, ok := localRuntime.Status[entry.Task.ID]; ok {
						item.Status = st.Status
						item.StartedMs = st.StartedMs
					}
				} else {
					rt := remoteRuntimes[execNode]
					if rt == nil {
						item.Unavailable = true
					} else {
						if st, ok := rt.Status[entry.Task.ID]; ok {
							item.Status = st.Status
							item.StartedMs = st.StartedMs
						}
						var next *int64
						for _, sch := range rt.Schedules {
							if sch.ID.String() == entry.Task.ID || string(sch.Topic) == entry.Task.ID {
								next = sch.NextMs
								break
							}
						}
						item.NextMs = next
					}
				}
				items = append(items, item)
			}

			var historyRuns []protocol.ScheduledTaskHistoryEntry
			var historyMore bool
			var historyErr error
			if panelTaskID != "" {
				target := panelExecNode
				if target == (protocol.NodeID{}) {
					target = localNode
				}
				historyRuns, historyMore, historyErr = ctrl.fetchHistory(ctx, target, panelTaskID, pages)
			}

			ctrl.mu.Lock()
			if ctrl.closed || ctrl.reloadGen != gen {
				ctrl.mu.Unlock()
				return
			}
			ctrl.refreshDirectoryLocked()
			ctrl.items = items
			ctrl.loaded = true
			if ctrl.panel != nil && !slices.ContainsFunc(items, func(it TaskItem) bool { return it.Task.ID == ctrl.panel.taskID }) {
				ctrl.panel = nil
			}
			currentPanelTask := ""
			if ctrl.panel != nil {
				currentPanelTask = ctrl.panel.taskID
			}
			if panelTaskID != "" && panelTaskID == currentPanelTask {
				if historyErr != nil {
					ctrl.history = nil
					ctrl.historyTaskID = panelTaskID
					ctrl.historyLoaded = false
					ctrl.historyMore = false
					if ctx.Err() == nil {
						ctrl.pushToastLocked(kit.Failed, "scheduled_tasks_execution_unavailable")
					}
				} else {
					ctrl.history = historyRuns
					ctrl.historyTaskID = panelTaskID
					ctrl.historyLoaded = true
					ctrl.historyMore = historyMore
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

func (ctrl *controller) executionNodeOf(task protocol.ScheduledTask) protocol.NodeID {
	if task.ExecutionNode != nil && *task.ExecutionNode != (protocol.NodeID{}) {
		return *task.ExecutionNode
	}
	return ctrl.services.Node
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

func (ctrl *controller) fetchAllTasks(ctx context.Context, node protocol.NodeID) ([]protocol.ScheduledTaskEntry, error) {
	var all []protocol.ScheduledTaskEntry
	var after *string
	for {
		out, err := ctrl.executeCommand(ctx, node, protocol.Command{
			Kind: "list_scheduled_tasks",
			Data: &protocol.CommandListScheduledTasksPayload{After: after},
		})
		if err != nil {
			return nil, err
		}
		page, ok := decodeOutput[protocol.ScheduledTaskPage](out)
		if !ok {
			return nil, fmt.Errorf("unexpected list_scheduled_tasks output %q", out.Kind)
		}
		all = append(all, page.Items...)
		if page.After == nil || *page.After == "" {
			break
		}
		nextAfter := *page.After
		after = &nextAfter
	}
	return all, nil
}

func (ctrl *controller) fetchRuntime(ctx context.Context, node protocol.NodeID) (protocol.ScheduledTaskRuntime, error) {
	out, err := ctrl.executeCommand(ctx, node, protocol.Command{Kind: "read_scheduled_task_runtime"})
	if err != nil {
		return protocol.ScheduledTaskRuntime{}, err
	}
	rt, ok := decodeOutput[protocol.ScheduledTaskRuntime](out)
	if !ok {
		return protocol.ScheduledTaskRuntime{}, fmt.Errorf("unexpected read_scheduled_task_runtime output %q", out.Kind)
	}
	return rt, nil
}

func (ctrl *controller) fetchHistory(ctx context.Context, node protocol.NodeID, taskID string, pages int) ([]protocol.ScheduledTaskHistoryEntry, bool, error) {
	var runs []protocol.ScheduledTaskHistoryEntry
	var before *uint64
	for range pages {
		handler := taskID
		execNode := node
		out, err := ctrl.executeCommand(ctx, node, protocol.Command{
			Kind: "read_scheduled_task_history",
			Data: &protocol.CommandReadScheduledTaskHistoryPayload{
				Before:        before,
				Handler:       &handler,
				Limit:         historyPageLimit,
				ExecutionNode: &execNode,
			},
		})
		if err != nil {
			return nil, false, err
		}
		page, ok := decodeOutput[protocol.ScheduledTaskHistory](out)
		if !ok {
			return nil, false, fmt.Errorf("unexpected read_scheduled_task_history output %q", out.Kind)
		}
		runs = append(runs, page.Items...)
		before = page.NextBefore
		if before == nil {
			break
		}
	}
	return runs, before != nil, nil
}

func (ctrl *controller) inspect(item TaskItem, tab string) {
	ctrl.mu.Lock()
	needReload := false
	if ctrl.panel == nil || ctrl.panel.taskID != item.Task.ID {
		ctrl.panel = &panelState{taskID: item.Task.ID, tab: tab}
		ctrl.history = nil
		ctrl.historyTaskID = ""
		ctrl.historyLoaded = false
		ctrl.historyPages = 1
		ctrl.historyMore = false
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

func (ctrl *controller) loadMoreHistory() {
	ctrl.mu.Lock()
	if ctrl.loading || !ctrl.historyMore {
		ctrl.mu.Unlock()
		return
	}
	ctrl.historyPages++
	ctrl.mu.Unlock()
	ctrl.reload()
}

func (ctrl *controller) openEditor(item *TaskItem) {
	ctrl.mu.Lock()
	if ctrl.busy || ctrl.pending != nil {
		ctrl.mu.Unlock()
		return
	}
	ctrl.refreshDirectoryLocked()
	ctrl.deleting = nil
	now := ctrl.now().In(ctrl.loc)
	defaultStart := now.Add(5 * time.Minute)
	var ed *editorState
	if item == nil {
		ed = &editorState{
			executionNode: ctrl.services.Node,
			enabled:       true,
			queue:         "default",
			execution:     protocol.ConversationExecutionModel,
			models:        map[protocol.RoleID]protocol.RoleModel{},
			timingKind:    "once",
			startDate:     defaultStart,
			startTime:     defaultStart,
			intervalSec:   "3600",
			weekdays:      []uint8{1, 2, 3, 4, 5},
			wallTime:      "09:00",
			timezone:      "",
		}
	} else {
		orig := canonicalTaskSnapshot(item.Task)
		var rev uint64
		if item.Revision != nil {
			rev = *item.Revision
		}
		execMode := item.Task.Execution
		if execMode == "" {
			execMode = protocol.ConversationExecutionTeam
		}
		var cfgCopy *protocol.SessionConfig
		modelID := ""
		effort := "default"
		if item.Task.Config.Model != "" && item.Task.Config.Provider != (protocol.ProviderID{}) {
			cCopy := cloneSessionConfig(item.Task.Config)
			cfgCopy = &cCopy
			modelID = item.Task.Config.Provider.String() + "/" + item.Task.Config.Model
			effort = effortString(item.Task.Config.Effort)
		}
		var teamCopy *protocol.TeamSelection
		if item.Task.Team != nil {
			teamCopy = &protocol.TeamSelection{ID: item.Task.Team.ID, Revision: nil}
		}
		ed = &editorState{
			editing:       true,
			original:      &orig,
			revision:      rev,
			taskID:        item.Task.ID,
			executionNode: ctrl.executionNodeOf(item.Task),
			name:          item.Task.Name,
			prompt:        item.Task.Prompt,
			enabled:       item.Task.Enabled,
			queue:         item.Task.Queue,
			project:       cloneProjectID(item.Task.Project),
			worktree:      cloneWorktreeID(item.Task.Worktree),
			execution:     execMode,
			team:          teamCopy,
			models:        maps.Clone(item.Task.Models),
			config:        cfgCopy,
			modelID:       modelID,
			effort:        effort,
			timingKind:    item.Task.Timing.Kind,
			startDate:     defaultStart,
			startTime:     defaultStart,
			intervalSec:   "3600",
			weekdays:      []uint8{1, 2, 3, 4, 5},
			wallTime:      "09:00",
			timezone:      "",
		}
		if ed.queue == "" {
			ed.queue = "default"
		}
		if ed.models == nil {
			ed.models = map[protocol.RoleID]protocol.RoleModel{}
		}
		if once, ok := timingOnce(item.Task.Timing); ok {
			t := time.UnixMilli(once.AtMs).In(ctrl.loc)
			ed.startDate, ed.startTime, ed.originalAt = t, t, once.AtMs
		} else if every, ok := timingEvery(item.Task.Timing); ok {
			t := time.UnixMilli(every.AnchorMs).In(ctrl.loc)
			ed.startDate, ed.startTime, ed.originalAt = t, t, every.AnchorMs
			ed.intervalSec = strconv.FormatUint(every.IntervalMs/1000, 10)
		} else if weekly, ok := timingWeekly(item.Task.Timing); ok {
			ed.weekdays = slices.Clone([]uint8(weekly.Weekdays))
			slices.Sort(ed.weekdays)
			ed.wallTime = FormatWallClock(weekly.Hour, weekly.Minute)
			ed.timezone = weekly.Timezone
		} else {
			ed.timingKind = "once"
		}
	}
	ctrl.syncEditorLeadModelLocked(ed)
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

func (ctrl *controller) requestDelete(item TaskItem) {
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
	expected := canonicalTaskSnapshot(target.Task)
	nodeID := ctrl.executionNodeOf(target.Task)
	cmd := protocol.Command{
		Kind: "remove_scheduled_task",
		Data: &protocol.CommandRemoveScheduledTaskPayload{
			ID:            target.Task.ID,
			Revision:      rev,
			ExpectedValue: &expected,
		},
	}
	ctrl.mu.Unlock()
	ctrl.startMutation(nodeID, cmd, "remove", false)
}

func (ctrl *controller) toggleEnabled(item TaskItem, enabled bool) {
	ctrl.mu.Lock()
	if ctrl.busy || ctrl.pending != nil || item.Unavailable {
		ctrl.mu.Unlock()
		return
	}
	var rev uint64
	if item.Revision != nil {
		rev = *item.Revision
	}
	expected := canonicalTaskSnapshot(item.Task)
	updated := expected
	updated.Enabled = enabled
	nodeID := ctrl.executionNodeOf(item.Task)
	cmd := protocol.Command{
		Kind: "save_scheduled_task",
		Data: &protocol.CommandSaveScheduledTaskPayload{
			Task:          updated,
			Revision:      rev,
			ExpectedValue: &expected,
		},
	}
	ctrl.mu.Unlock()
	ctrl.startMutation(nodeID, cmd, "save", false)
}

func (ctrl *controller) runNow(item TaskItem) {
	ctrl.mu.Lock()
	if ctrl.busy || ctrl.pending != nil || item.Unavailable {
		ctrl.mu.Unlock()
		return
	}
	expected := canonicalTaskSnapshot(item.Task)
	nodeID := ctrl.executionNodeOf(item.Task)
	cmd := protocol.Command{
		Kind: "run_scheduled_task",
		Data: &protocol.CommandRunScheduledTaskPayload{
			ID:            item.Task.ID,
			Revision:      item.Revision,
			ExpectedValue: &expected,
		},
	}
	ctrl.mu.Unlock()
	ctrl.startMutation(nodeID, cmd, "run", false)
}

func (ctrl *controller) cancelOrStopRun(run protocol.ScheduledTaskHistoryEntry, fallbackNode protocol.NodeID) {
	ctrl.mu.Lock()
	if ctrl.busy || ctrl.pending != nil {
		ctrl.mu.Unlock()
		return
	}
	target := run.ExecutionNode
	if target == (protocol.NodeID{}) {
		target = fallbackNode
	}
	var cmd protocol.Command
	var kind string
	switch {
	case run.Job.Status == protocol.DispatchStatusQueued:
		execNode := target
		kind = "cancel"
		cmd = protocol.Command{
			Kind: "cancel_scheduled_task_job",
			Data: &protocol.CommandCancelScheduledTaskJobPayload{ID: run.Job.ID, ExecutionNode: &execNode},
		}
	case run.Job.Status == protocol.DispatchStatusRunning && run.Turn != nil:
		execNode := target
		kind = "stop"
		cmd = protocol.Command{
			Kind: "stop_scheduled_task_job",
			Data: &protocol.CommandStopScheduledTaskJobPayload{ID: run.Job.ID, ExecutionNode: &execNode},
		}
	default:
		ctrl.mu.Unlock()
		return
	}
	ctrl.mu.Unlock()
	ctrl.startMutation(target, cmd, kind, false)
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
	task, errKey := ctrl.buildTaskFromEditorLocked(ed)
	if errKey != "" {
		ctrl.pushToastLocked(kit.Failed, errKey)
		ctrl.mu.Unlock()
		ctrl.wake()
		return
	}
	payload := &protocol.CommandSaveScheduledTaskPayload{
		Task:     task,
		Revision: ed.revision,
	}
	if ed.original != nil {
		origCopy := *ed.original
		payload.ExpectedValue = &origCopy
	}
	target := ed.executionNode
	ctrl.mu.Unlock()
	ctrl.startMutation(target, protocol.Command{Kind: "save_scheduled_task", Data: payload}, "save", true)
}

func (ctrl *controller) buildTaskFromEditorLocked(ed *editorState) (protocol.ScheduledTask, string) {
	name := strings.TrimSpace(ed.name)
	if name == "" || strings.TrimSpace(ed.prompt) == "" {
		return protocol.ScheduledTask{}, "scheduled_tasks_required"
	}
	ctrl.syncEditorLeadModelLocked(ed)
	if ed.config == nil || ed.modelID == "" {
		return protocol.ScheduledTask{}, "scheduled_tasks_model_required"
	}

	var timing protocol.DispatchTiming
	nowMs := ctrl.now().UnixMilli()
	switch ed.timingKind {
	case "weekly":
		hour, minute, ok := ParseWallClock(ed.wallTime)
		if !ok {
			return protocol.ScheduledTask{}, "scheduled_tasks_invalid_clock"
		}
		tz := strings.TrimSpace(ed.timezone)
		if tz == "" {
			return protocol.ScheduledTask{}, "scheduled_tasks_timezone_required"
		}
		if len(ed.weekdays) == 0 {
			return protocol.ScheduledTask{}, "scheduled_tasks_weekdays_required"
		}
		days := slices.Clone(ed.weekdays)
		slices.Sort(days)
		timing = protocol.DispatchTiming{
			Kind: "weekly",
			Data: protocol.DispatchTimingWeeklyPayload{
				Timezone: tz,
				Weekdays: protocol.Bytes(days),
				Hour:     hour,
				Minute:   minute,
			},
		}
	case "every":
		atMs, ok := CombineLocalDateTime(ed.startDate, ed.startTime, ed.originalAt, ctrl.loc)
		if !ok {
			return protocol.ScheduledTask{}, "scheduled_tasks_invalid_time"
		}
		if !ed.editing && atMs <= nowMs {
			return protocol.ScheduledTask{}, "scheduled_tasks_invalid_time"
		}
		sec, err := strconv.ParseFloat(strings.TrimSpace(ed.intervalSec), 64)
		intervalMs := int64(sec * 1000)
		if err != nil || intervalMs < 1 {
			return protocol.ScheduledTask{}, "scheduled_tasks_invalid_interval"
		}
		timing = protocol.DispatchTiming{
			Kind: "every",
			Data: protocol.DispatchTimingEveryPayload{
				AnchorMs:   atMs,
				IntervalMs: uint64(intervalMs),
			},
		}
	default:
		atMs, ok := CombineLocalDateTime(ed.startDate, ed.startTime, ed.originalAt, ctrl.loc)
		if !ok {
			return protocol.ScheduledTask{}, "scheduled_tasks_invalid_time"
		}
		if !ed.editing && atMs <= nowMs {
			return protocol.ScheduledTask{}, "scheduled_tasks_invalid_time"
		}
		timing = protocol.DispatchTiming{
			Kind: "once",
			Data: protocol.DispatchTimingOncePayload{AtMs: atMs},
		}
	}

	execNode := ed.executionNode
	var teamSel *protocol.TeamSelection
	models := map[protocol.RoleID]protocol.RoleModel{}
	if ed.execution == protocol.ConversationExecutionTeam {
		if ed.team != nil {
			teamSel = &protocol.TeamSelection{ID: ed.team.ID, Revision: nil}
		}
		if ed.models != nil {
			models = maps.Clone(ed.models)
		}
	}
	queue := ed.queue
	if queue == "" {
		queue = "default"
	}
	return protocol.ScheduledTask{
		ID:            ed.taskID,
		ExecutionNode: &execNode,
		Name:          name,
		Prompt:        ed.prompt,
		Project:       cloneProjectID(ed.project),
		Worktree:      cloneWorktreeID(ed.worktree),
		Config:        cloneSessionConfig(*ed.config),
		Execution:     ed.execution,
		Team:          teamSel,
		Models:        models,
		Timing:        timing,
		Enabled:       ed.enabled,
		Queue:         queue,
	}, ""
}

func (ctrl *controller) startMutation(target protocol.NodeID, cmd protocol.Command, kind string, closeEditor bool) {
	ctrl.mu.Lock()
	if ctrl.closed || ctrl.busy || ctrl.pending != nil {
		ctrl.mu.Unlock()
		return
	}
	if !ctrl.nodeConnectedLocked(target) {
		ctrl.pushToastLocked(kit.Failed, "scheduled_tasks_execution_unavailable")
		ctrl.mu.Unlock()
		ctrl.wake()
		return
	}
	req, err := ctrl.prepareCommand(target, cmd)
	if err != nil {
		ctrl.pushToastLocked(kit.Failed, "scheduled_tasks_execution_unavailable")
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
		ctrl.pushToastLocked(kit.Failed, "scheduled_tasks_execution_unavailable")
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
		key := "scheduled_tasks_unknown"
		if code == protocol.ErrorCodeUnavailable {
			key = "scheduled_tasks_execution_unavailable"
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
	key := "scheduled_tasks_failed"
	switch code {
	case protocol.ErrorCodeRevisionConflict:
		outcome = kit.Conflict
		key = "scheduled_tasks_conflict"
	case protocol.ErrorCodeNotConfigured:
		key = "scheduled_tasks_not_configured"
	case protocol.ErrorCodeUnavailable:
		key = "scheduled_tasks_execution_unavailable"
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

func (ctrl *controller) projectNameLocked(id *protocol.ProjectID, t Translate) string {
	if id == nil {
		return t("scheduled_tasks_no_project")
	}
	for _, pr := range ctrl.projects {
		if pr.Project.ID == *id {
			return pr.Project.Name
		}
	}
	return t("scheduled_tasks_unavailable")
}

func (ctrl *controller) worktreePathLocked(id *protocol.WorktreeID, t Translate) string {
	if id == nil {
		return t("scheduled_tasks_main_worktree")
	}
	for _, wt := range ctrl.worktrees {
		if wt.Worktree.ID == *id {
			return wt.Worktree.Path
		}
	}
	return t("scheduled_tasks_unavailable")
}

func (ctrl *controller) teamNameLocked(node protocol.NodeID, sel *protocol.TeamSelection, t Translate) string {
	if sel == nil {
		return t("scheduled_tasks_default_team")
	}
	for _, team := range ctrl.teamsBy[node] {
		if team.ID == sel.ID {
			return team.Name
		}
	}
	return t("scheduled_tasks_unavailable")
}

func (ctrl *controller) fixedLeadModelLocked(ed *editorState) *protocol.RoleModel {
	if ed == nil || ed.execution != protocol.ConversationExecutionTeam || ed.team == nil {
		return nil
	}
	for _, team := range ctrl.teamsBy[ed.executionNode] {
		if team.ID == ed.team.ID {
			return team.Model
		}
	}
	return nil
}

func (ctrl *controller) syncEditorLeadModelLocked(ed *editorState) {
	fixed := ctrl.fixedLeadModelLocked(ed)
	if fixed == nil {
		return
	}
	modelID := fixed.Provider.String() + "/" + fixed.Model
	effort := ""
	if fixed.Effort != nil {
		effort = effortString(*fixed.Effort)
	}
	if cfg, ok := ctrl.resolveConfigFromSnapshotLocked(ed.executionNode, modelID, effort, ed.config); ok {
		ed.config = &cfg
		ed.modelID = modelID
		ed.effort = effortString(cfg.Effort)
	}
}

func (ctrl *controller) selectEditorModelLocked(ed *editorState, modelID, effort string) bool {
	if ed == nil || ctrl.fixedLeadModelLocked(ed) != nil {
		return false
	}
	cfg, ok := ctrl.resolveConfigFromSnapshotLocked(ed.executionNode, modelID, effort, ed.config)
	if !ok {
		return false
	}
	ed.config = &cfg
	ed.modelID = modelID
	ed.effort = effortString(cfg.Effort)
	return true
}

func (ctrl *controller) resolveConfigFromSnapshotLocked(node protocol.NodeID, selected, effort string, base *protocol.SessionConfig) (protocol.SessionConfig, bool) {
	provStr, modelName, found := strings.Cut(selected, "/")
	if !found || modelName == "" {
		return protocol.SessionConfig{}, false
	}
	provID, err := protocol.ParseProviderID(provStr)
	if err != nil {
		return protocol.SessionConfig{}, false
	}
	for _, prov := range ctrl.providers[node] {
		if !prov.Enabled || prov.ID != provID {
			continue
		}
		idx := slices.IndexFunc(prov.Models, func(m protocol.ConversationModel) bool { return m.ID == modelName })
		if idx < 0 {
			continue
		}
		model := prov.Models[idx]
		cfg := protocol.DefaultSessionConfig()
		cfg.Mode = protocol.WorkModeCode
		cfg.Permission = protocol.PermissionAsk
		cfg.Provider = prov.ID
		cfg.Model = model.ID
		cfg.Credential = prov.Credential
		cfg.Effort = model.DefaultEffort
		if len(model.Efforts) > 0 && !slices.ContainsFunc(model.Efforts, func(e protocol.Effort) bool { return reflect.DeepEqual(e, cfg.Effort) }) {
			cfg.Effort = model.Efforts[0]
		}
		if base != nil {
			unchanged := base.Provider == cfg.Provider && base.Model == cfg.Model
			sel := cfg
			cfg = cloneSessionConfig(*base)
			cfg.Provider = sel.Provider
			cfg.Model = sel.Model
			cfg.Credential = sel.Credential
			if !unchanged {
				cfg.Features = map[string]string{}
				cfg.Effort = sel.Effort
			}
		}
		if effort != "" {
			parsed := parseEffortString(effort)
			if len(model.Efforts) == 0 || slices.ContainsFunc(model.Efforts, func(e protocol.Effort) bool { return reflect.DeepEqual(e, parsed) }) {
				cfg.Effort = parsed
			}
		}
		return cfg, true
	}
	return protocol.SessionConfig{}, false
}

func (ctrl *controller) modelOptionsLocked(node protocol.NodeID) []kit.ModelOption {
	if ctrl.services.ModelOptions != nil {
		return ctrl.services.ModelOptions(node)
	}
	provs := ctrl.providers[node]
	var out []kit.ModelOption
	for _, prov := range provs {
		if !prov.Enabled {
			continue
		}
		mark := providerBrandMark(prov)
		family := providerFamily(prov.Api)
		for _, m := range prov.Models {
			name := m.ID
			if m.DisplayName != nil && *m.DisplayName != "" {
				name = *m.DisplayName
			}
			var efforts []string
			if m.Reasoning {
				for _, e := range m.Efforts {
					efforts = append(efforts, effortString(e))
				}
			}
			out = append(out, kit.ModelOption{
				ID:            prov.ID.String() + "/" + m.ID,
				Name:          name,
				Provider:      prov.Name,
				Mark:          mark,
				Brand:         family,
				Efforts:       efforts,
				DefaultEffort: effortString(m.DefaultEffort),
			})
		}
	}
	return out
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
		if n.copied {
			kit.Copied(c)
			continue
		}
		kit.Toast(c, n.outcome, i18n.T(c, n.key))
	}
}

func (ctrl *controller) slots() shell.WorkspaceSlots {
	ctrl.mu.Lock()
	hasPanel := ctrl.panel != nil && slices.ContainsFunc(ctrl.items, func(it TaskItem) bool { return it.Task.ID == ctrl.panel.taskID })
	ctrl.mu.Unlock()
	if !hasPanel {
		return shell.WorkspaceSlots{
			PreferenceKey:       string(PageID),
			DefaultDetailsWidth: detailDefaultWidth,
			MinDetailsWidth:     detailMinWidth,
			DetailsLabel:        "scheduled_tasks_details",
		}
	}
	return shell.WorkspaceSlots{
		PreferenceKey:       string(PageID),
		DefaultDetailsWidth: detailDefaultWidth,
		MinDetailsWidth:     detailMinWidth,
		DetailsLabel:        "scheduled_tasks_details",
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

func canonicalTaskSnapshot(task protocol.ScheduledTask) protocol.ScheduledTask {
	cp := task
	if task.ExecutionNode != nil {
		n := *task.ExecutionNode
		cp.ExecutionNode = &n
	}
	cp.Project = cloneProjectID(task.Project)
	cp.Worktree = cloneWorktreeID(task.Worktree)
	cp.Config = cloneSessionConfig(task.Config)
	if cp.Execution == "" {
		cp.Execution = protocol.ConversationExecutionTeam
	}
	if task.Team != nil {
		cp.Team = &protocol.TeamSelection{ID: task.Team.ID, Revision: nil}
	}
	if task.Models != nil {
		cp.Models = maps.Clone(task.Models)
	} else {
		cp.Models = map[protocol.RoleID]protocol.RoleModel{}
	}
	if weekly, ok := timingWeekly(task.Timing); ok {
		days := slices.Clone([]byte(weekly.Weekdays))
		slices.Sort(days)
		weekly.Weekdays = protocol.Bytes(days)
		cp.Timing = protocol.DispatchTiming{Kind: "weekly", Data: weekly}
	}
	return cp
}

func cloneSessionConfig(cfg protocol.SessionConfig) protocol.SessionConfig {
	cp := cfg
	if cfg.Credential != nil {
		c := *cfg.Credential
		cp.Credential = &c
	}
	if cfg.Features != nil {
		cp.Features = maps.Clone(cfg.Features)
	} else {
		cp.Features = map[string]string{}
	}
	if cfg.Options != nil {
		cp.Options = make(map[string]map[string]json.RawMessage, len(cfg.Options))
		for k, v := range cfg.Options {
			cp.Options[k] = maps.Clone(v)
		}
	} else {
		cp.Options = map[string]map[string]json.RawMessage{}
	}
	return cp
}

func cloneProjectID(id *protocol.ProjectID) *protocol.ProjectID {
	if id == nil {
		return nil
	}
	cp := *id
	return &cp
}

func cloneWorktreeID(id *protocol.WorktreeID) *protocol.WorktreeID {
	if id == nil {
		return nil
	}
	cp := *id
	return &cp
}

func effortString(e protocol.Effort) string {
	if e.Kind == "budget" {
		switch v := e.Data.(type) {
		case int32:
			return strconv.FormatInt(int64(v), 10)
		case *int32:
			if v != nil {
				return strconv.FormatInt(int64(*v), 10)
			}
		}
	}
	if e.Kind == "" {
		return "default"
	}
	return e.Kind
}

func parseEffortString(s string) protocol.Effort {
	if n, err := strconv.ParseInt(s, 10, 32); err == nil {
		return protocol.Effort{Kind: "budget", Data: int32(n)}
	}
	if s == "" {
		return protocol.Effort{Kind: "default"}
	}
	return protocol.Effort{Kind: s}
}

func providerFamily(api protocol.ConversationModelApi) string {
	switch api.Family() {
	case "anthropic":
		return "anthropic"
	case "responses", "chat_completions", "azure_open_ai":
		return "openai"
	case "gemini":
		return "gemini"
	default:
		return "other"
	}
}

func providerBrandMark(prov protocol.ConversationProvider) *ui.SVG {
	if prov.Preset != nil {
		if m := icons.Brand(prov.Preset.ID); m != nil {
			return m
		}
	}
	return icons.Brand(providerFamily(prov.Api))
}
