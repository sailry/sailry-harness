package scheduledtasksnode

import (
	"context"
	"encoding/json"
	"errors"
	"maps"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type storageClient struct {
	host moduleapi.Host
	inv  *store.AgentInvocation
}

func (c storageClient) get(ctx context.Context, key string) (protocol.PluginStorageEntry, error) {
	if c.inv != nil {
		return c.host.PluginStorageGet(ctx, c.inv, storageNamespace, key)
	}
	return c.host.StorageGet(ctx, storageNamespace, key)
}

func (c storageClient) listKeys(ctx context.Context, prefix string, after *string, limit uint16) (protocol.PluginStoragePage, error) {
	if c.inv != nil {
		return c.host.PluginStorageListKeys(ctx, c.inv, storageNamespace, prefix, after, limit)
	}
	return c.host.StorageListKeys(ctx, storageNamespace, prefix, after, limit)
}

func (c storageClient) transaction(ctx context.Context, ops []protocol.PluginTransactionOperation) ([]protocol.Output, error) {
	if c.inv != nil {
		return c.host.PluginStorageTransaction(ctx, c.inv, storageNamespace, ops)
	}
	return c.host.StorageTransaction(ctx, storageNamespace, ops)
}

func (c storageClient) dispatch(ctx context.Context, action protocol.DispatchCommand) (protocol.DispatchOutput, error) {
	if c.inv != nil {
		return c.host.PluginStorageDispatch(ctx, c.inv, storageNamespace, action)
	}
	return c.host.StorageDispatch(ctx, storageNamespace, action)
}

func (c storageClient) listSchedules(ctx context.Context) ([]protocol.DispatchSchedule, error) {
	out, err := c.dispatch(ctx, protocol.DispatchCommand{Kind: "list_schedules"})
	if err != nil {
		return nil, err
	}
	switch v := out.Data.(type) {
	case []protocol.DispatchSchedule:
		if v == nil {
			return []protocol.DispatchSchedule{}, nil
		}
		return v, nil
	case *[]protocol.DispatchSchedule:
		if v != nil && *v != nil {
			return *v, nil
		}
		return []protocol.DispatchSchedule{}, nil
	}
	raw, err := json.Marshal(out.Data)
	if err != nil {
		return nil, err
	}
	var items []protocol.DispatchSchedule
	if err := json.Unmarshal(raw, &items); err != nil {
		return nil, err
	}
	if items == nil {
		items = []protocol.DispatchSchedule{}
	}
	return items, nil
}

func (c storageClient) listHandlers(ctx context.Context) ([]protocol.DispatchHandler, error) {
	out, err := c.dispatch(ctx, protocol.DispatchCommand{Kind: "list_handlers"})
	if err != nil {
		return nil, err
	}
	switch v := out.Data.(type) {
	case []protocol.DispatchHandler:
		return v, nil
	case *[]protocol.DispatchHandler:
		if v != nil {
			return *v, nil
		}
	}
	raw, err := json.Marshal(out.Data)
	if err != nil {
		return nil, err
	}
	var items []protocol.DispatchHandler
	if err := json.Unmarshal(raw, &items); err != nil {
		return nil, err
	}
	return items, nil
}

func (c storageClient) listJobs(ctx context.Context, before *uint64, limit uint16) (protocol.DispatchPage, error) {
	out, err := c.dispatch(ctx, protocol.DispatchCommand{
		Kind: "list_jobs",
		Data: &protocol.DispatchCommandListJobsPayload{Before: before, Limit: limit},
	})
	if err != nil {
		return protocol.DispatchPage{}, err
	}
	switch v := out.Data.(type) {
	case protocol.DispatchPage:
		return v, nil
	case *protocol.DispatchPage:
		if v != nil {
			return *v, nil
		}
	}
	raw, err := json.Marshal(out.Data)
	if err != nil {
		return protocol.DispatchPage{}, err
	}
	var page protocol.DispatchPage
	if err := json.Unmarshal(raw, &page); err != nil {
		return protocol.DispatchPage{}, err
	}
	return page, nil
}

func (c storageClient) readJobResult(ctx context.Context, id protocol.JobID) (protocol.RequestOutcome, error) {
	out, err := c.dispatch(ctx, protocol.DispatchCommand{
		Kind: "read_result",
		Data: &protocol.DispatchCommandReadResultPayload{ID: id},
	})
	if err != nil {
		return protocol.RequestOutcome{}, err
	}
	switch v := out.Data.(type) {
	case protocol.RequestOutcome:
		return v, nil
	case *protocol.RequestOutcome:
		if v != nil {
			return *v, nil
		}
	}
	raw, err := json.Marshal(out.Data)
	if err != nil {
		return protocol.RequestOutcome{}, err
	}
	var outcome protocol.RequestOutcome
	if err := json.Unmarshal(raw, &outcome); err != nil {
		return protocol.RequestOutcome{}, err
	}
	return outcome, nil
}

func (c storageClient) cancelJob(ctx context.Context, id protocol.JobID) (protocol.DispatchJob, error) {
	out, err := c.dispatch(ctx, protocol.DispatchCommand{
		Kind: "cancel_job",
		Data: &protocol.DispatchCommandCancelJobPayload{ID: id},
	})
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	job, ok := unwrapDispatchJob(out.Data)
	if !ok {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task cancellation outcome is unavailable")
	}
	return job, nil
}

type storedRecord struct {
	present  bool
	revision uint64
	value    protocol.ScheduledTask
}

func (c storageClient) read(ctx context.Context, id string) (storedRecord, error) {
	k, err := taskKey(id)
	if err != nil {
		return storedRecord{}, err
	}
	entry, err := c.get(ctx, k)
	if err != nil {
		return storedRecord{}, err
	}
	if !entry.Present {
		return storedRecord{present: false, revision: entry.Revision}, nil
	}
	parsed, err := parseStoredTask(entry.Value, id)
	if err != nil {
		return storedRecord{}, err
	}
	return storedRecord{present: true, revision: entry.Revision, value: parsed}, nil
}

// executionNode resolves the execution Node owning item. Legacy records have no
// execution_node field; only local dispatch records prove local ownership.
func (c storageClient) executionNode(ctx context.Context, item protocol.ScheduledTask) (protocol.NodeID, error) {
	if item.ExecutionNode != nil {
		if err := validateNodeID(item.ExecutionNode); err != nil {
			return protocol.NodeID{}, err
		}
		return *item.ExecutionNode, nil
	}
	schedules, err := c.listSchedules(ctx)
	if err != nil {
		return protocol.NodeID{}, err
	}
	for _, s := range schedules {
		if s.ID.String() == item.ID {
			return c.host.LocalNodeID(), nil
		}
	}
	handlers, err := c.listHandlers(ctx)
	if err != nil {
		return protocol.NodeID{}, err
	}
	for _, h := range handlers {
		if h.Name == item.ID {
			return c.host.LocalNodeID(), nil
		}
	}
	return protocol.NodeID{}, fault(protocol.ErrorCodeUnavailable, "Task execution Node is unknown")
}

// resolveConfig resolves the task's model configuration against live Node
// providers and, when a named team fixes a lead model, resolves that lead model
// (including its owner's default effort when the team omits effort) onto the
// base session config.
func (c storageClient) resolveConfig(ctx context.Context, cfg protocol.SessionConfig, team *protocol.TeamSelection) (protocol.SessionConfig, error) {
	if err := requireConfig(cfg); err != nil {
		return protocol.SessionConfig{}, err
	}
	var fixed *protocol.RoleModel
	if team != nil {
		teams, err := c.host.ListTeams(ctx)
		if err != nil {
			return protocol.SessionConfig{}, err
		}
		var found *protocol.TeamProfile
		for i := range teams {
			if teams[i].ID == team.ID {
				found = &teams[i]
				break
			}
		}
		if found == nil {
			return protocol.SessionConfig{}, fault(protocol.ErrorCodeNotFound, "Task team is unavailable")
		}
		fixed = found.Model
	}
	base := cfg
	if fixed == nil {
		modelSpec := cfg.Provider.String() + "/" + cfg.Model
		effort := cfg.Effort
		return c.host.ResolveModel(ctx, modelSpec, &effort, &base)
	}
	modelSpec := fixed.Provider.String() + "/" + fixed.Model
	effort := fixed.Effort
	if effort == nil {
		// Resolve the model's default effort at its owner, even when the base uses that model.
		ownerDefault, err := c.host.ResolveModel(ctx, modelSpec, nil, nil)
		if err != nil {
			return protocol.SessionConfig{}, err
		}
		e := ownerDefault.Effort
		effort = &e
	}
	return c.host.ResolveModel(ctx, modelSpec, effort, &base)
}

func isOfflineFault(err error) bool {
	var f *protocol.Fault
	if !errors.As(err, &f) {
		return false
	}
	switch f.Code {
	case protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy, protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeInternal, protocol.ErrorCodeCancelled:
		return true
	}
	return false
}

func isMissingOrDeniedFault(err error) bool {
	var f *protocol.Fault
	if !errors.As(err, &f) {
		return false
	}
	return f.Code == protocol.ErrorCodePermissionDenied || f.Code == protocol.ErrorCodeNotFound
}

func (c storageClient) visible(ctx context.Context, id string) (*protocol.ScheduledTaskEntry, error) {
	rec, err := c.read(ctx, id)
	if err != nil {
		return nil, err
	}
	if !rec.present {
		return nil, nil
	}
	local := c.host.LocalNodeID()
	if rec.value.ExecutionNode == nil || sameNode(*rec.value.ExecutionNode, local) {
		rev := rec.revision
		return &protocol.ScheduledTaskEntry{Task: rec.value, Revision: &rev}, nil
	}
	out, err := c.host.ForwardModuleCommand(ctx, *rec.value.ExecutionNode, protocol.Command{
		Kind: "read_scheduled_task",
		Data: &protocol.CommandReadScheduledTaskPayload{ID: id},
	})
	if err != nil {
		if isMissingOrDeniedFault(err) {
			return nil, nil
		}
		if isOfflineFault(err) {
			return &protocol.ScheduledTaskEntry{Task: rec.value, Revision: nil, Unavailable: true}, nil
		}
		return nil, err
	}
	entry, ok := unwrapScheduledTaskEntry(out.Data)
	if !ok {
		return nil, nil
	}
	return &entry, nil
}

func (c storageClient) listing(ctx context.Context, after *string, scopeProject *protocol.ProjectID, scoped bool) (protocol.ScheduledTaskPage, error) {
	var cursor *string
	if after != nil && *after != "" {
		cp := *after
		cursor = &cp
	}
	batch, err := c.listKeys(ctx, keyPrefix, cursor, maxPageItems)
	if err != nil {
		return protocol.ScheduledTaskPage{}, err
	}
	schedules, err := c.listSchedules(ctx)
	if err != nil {
		return protocol.ScheduledTaskPage{}, err
	}
	nextByID := make(map[string]*int64, len(schedules))
	for _, s := range schedules {
		nextByID[s.ID.String()] = s.NextMs
	}
	items := []protocol.ScheduledTaskEntry{}
	size := 0
	lastAfter := cursor
	for _, name := range batch.Keys {
		id := strings.TrimPrefix(name, keyPrefix)
		entry, err := c.visible(ctx, id)
		if err != nil {
			return protocol.ScheduledTaskPage{}, err
		}
		if entry != nil && (!scoped || sameProject(entry.Task.Project, scopeProject)) {
			if entry.NextMs == nil {
				entry.NextMs = nextByID[entry.Task.ID]
			}
			encoded, err := json.Marshal(entry)
			if err != nil {
				return protocol.ScheduledTaskPage{}, err
			}
			if len(items) > 0 && size+len(encoded) > maxPageBytes {
				return protocol.ScheduledTaskPage{Items: items, After: lastAfter}, nil
			}
			items = append(items, *entry)
			size += len(encoded)
		}
		cp := name
		lastAfter = &cp
	}
	return protocol.ScheduledTaskPage{Items: items, After: batch.After}, nil
}

func (c storageClient) handlerOperation(task protocol.ScheduledTask, handlerRev uint64) protocol.PluginTransactionOperation {
	var scopeWorktree *protocol.WorktreeID
	var scopeSession *protocol.SessionID
	if c.inv != nil {
		w := c.inv.Run.Worktree
		s := c.inv.Session.ID
		scopeWorktree = &w
		scopeSession = &s
	}
	cfg := task.Config
	models := map[protocol.RoleID]protocol.RoleModel{}
	if len(task.Models) > 0 {
		models = maps.Clone(task.Models)
	}
	var team *protocol.TeamSelection
	if task.Team != nil {
		team = &protocol.TeamSelection{ID: task.Team.ID, Revision: nil}
	}
	start := protocol.ConversationStart{
		Project:   task.Project,
		Worktree:  task.Worktree,
		Config:    &cfg,
		Execution: task.Execution,
		Team:      team,
		Models:    models,
		Association: &protocol.ConversationAssociation{
			Namespace: storageNamespace,
			Kind:      "task",
			ID:        task.ID,
		},
		Title: task.Name,
		Message: protocol.ConversationInput{
			Text:        task.Prompt,
			Attachments: []protocol.AttachmentID{},
			References:  []protocol.ConversationReference{},
		},
	}
	return protocol.PluginTransactionOperation{
		Kind: "dispatch",
		Data: protocol.DispatchCommand{
			Kind: "save_handler",
			Data: protocol.DispatchHandler{
				Name:     task.ID,
				Revision: handlerRev,
				Enabled:  true,
				Source:   protocol.DispatchSource{Package: storageNamespace, Topic: task.ID},
				Queue:    task.Queue,
				Callback: protocol.DispatchCallback{
					Scope:      protocol.DispatchScope{Worktree: scopeWorktree, Session: scopeSession},
					Completion: protocol.DispatchCompletionTurn,
					Bindings:   map[string]string{},
					Command: &protocol.Command{
						Kind: "start_session",
						Data: &start,
					},
				},
			},
		},
	}
}

func (c storageClient) scheduleChanges(ctx context.Context, task protocol.ScheduledTask) ([]protocol.PluginTransactionOperation, error) {
	if err := requireConfig(task.Config); err != nil {
		return nil, err
	}
	schedules, err := c.listSchedules(ctx)
	if err != nil {
		return nil, err
	}
	handlers, err := c.listHandlers(ctx)
	if err != nil {
		return nil, err
	}
	var scheduleRev uint64
	for i := range schedules {
		if schedules[i].ID.String() == task.ID {
			scheduleRev = schedules[i].Revision
			break
		}
	}
	var handlerRev uint64
	for i := range handlers {
		if handlers[i].Name == task.ID {
			handlerRev = handlers[i].Revision
			break
		}
	}
	scheduleID, err := protocol.ParseScheduleID(task.ID)
	if err != nil {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task ID")
	}
	payloadRaw, err := marshalRaw(map[string]string{"name": task.Name})
	if err != nil {
		return nil, err
	}
	return []protocol.PluginTransactionOperation{
		c.handlerOperation(task, handlerRev),
		{
			Kind: "dispatch",
			Data: protocol.DispatchCommand{
				Kind: "save_schedule",
				Data: protocol.DispatchSchedule{
					ID:       scheduleID,
					Revision: scheduleRev,
					Enabled:  task.Enabled,
					Topic:    task.ID,
					Payload:  payloadRaw,
					Timing:   task.Timing,
					NextMs:   nil,
				},
			},
		},
	}, nil
}

func (c storageClient) scheduleRemovals(ctx context.Context, id string) ([]protocol.PluginTransactionOperation, error) {
	schedules, err := c.listSchedules(ctx)
	if err != nil {
		return nil, err
	}
	handlers, err := c.listHandlers(ctx)
	if err != nil {
		return nil, err
	}
	scheduleID, err := protocol.ParseScheduleID(id)
	if err != nil {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task ID")
	}
	var ops []protocol.PluginTransactionOperation
	for _, s := range schedules {
		if s.ID == scheduleID {
			ops = append(ops, protocol.PluginTransactionOperation{
				Kind: "dispatch",
				Data: protocol.DispatchCommand{
					Kind: "remove_schedule",
					Data: protocol.DispatchCommandRemoveSchedulePayload{
						ID:               scheduleID,
						ExpectedRevision: s.Revision,
					},
				},
			})
			break
		}
	}
	for _, h := range handlers {
		if h.Name == id {
			ops = append(ops, protocol.PluginTransactionOperation{
				Kind: "dispatch",
				Data: protocol.DispatchCommand{
					Kind: "remove_handler",
					Data: protocol.DispatchCommandRemoveHandlerPayload{
						Name:             id,
						ExpectedRevision: h.Revision,
						CancelPending:    true,
					},
				},
			})
			break
		}
	}
	return ops, nil
}

func (c storageClient) saveOperations(ctx context.Context, input protocol.ScheduledTask, rev uint64, expectedVal *protocol.ScheduledTask, scopeProject *protocol.ProjectID, scoped bool) (protocol.ScheduledTask, []protocol.PluginTransactionOperation, error) {
	local := c.host.LocalNodeID()
	item, err := normalizeDraft(input, local, true)
	if err != nil {
		return protocol.ScheduledTask{}, nil, err
	}
	if scoped && !sameProject(item.Project, scopeProject) {
		return protocol.ScheduledTask{}, nil, fault(protocol.ErrorCodePermissionDenied, "Task belongs to another project")
	}
	old, err := c.read(ctx, item.ID)
	if err != nil {
		return protocol.ScheduledTask{}, nil, err
	}
	if old.present && scoped && !sameProject(old.value.Project, scopeProject) {
		return protocol.ScheduledTask{}, nil, fault(protocol.ErrorCodePermissionDenied, "Task belongs to another project")
	}
	expectedRev, err := checkBase(rev, expectedVal, old.present, old.revision, old.value)
	if err != nil {
		return protocol.ScheduledTask{}, nil, err
	}
	var target protocol.NodeID
	if old.present {
		target, err = c.executionNode(ctx, old.value)
		if err != nil {
			return protocol.ScheduledTask{}, nil, err
		}
	} else {
		target = *item.ExecutionNode
	}
	if !sameNode(target, local) || item.ExecutionNode == nil || !sameNode(*item.ExecutionNode, target) {
		return protocol.ScheduledTask{}, nil, fault(protocol.ErrorCodeConflict, "Task remains on its original execution Node")
	}
	item.ExecutionNode = &target
	resolved, err := c.resolveConfig(ctx, item.Config, item.Team)
	if err != nil {
		return protocol.ScheduledTask{}, nil, err
	}
	item.Config = resolved
	// Management commands validate against the Node-wide catalog; agent tools
	// keep their admitted turn scope and Node validates resources when firing.
	if !scoped && item.Project != nil {
		catalog, err := c.host.ReadProjectCatalog(ctx)
		if err != nil {
			return protocol.ScheduledTask{}, nil, err
		}
		projectFound := false
		for _, p := range catalog.Projects {
			if p.ID == *item.Project {
				projectFound = true
				break
			}
		}
		worktreeFound := item.Worktree == nil
		if projectFound && item.Worktree != nil {
			for _, tree := range catalog.Worktrees {
				if tree.ID == *item.Worktree && tree.Project != nil && *tree.Project == *item.Project {
					worktreeFound = true
					break
				}
			}
		}
		if !projectFound || !worktreeFound {
			return protocol.ScheduledTask{}, nil, fault(protocol.ErrorCodeNotFound, "Task project or worktree is unavailable")
		}
	}
	k, err := taskKey(item.ID)
	if err != nil {
		return protocol.ScheduledTask{}, nil, err
	}
	encoded, err := marshalRaw(item)
	if err != nil {
		return protocol.ScheduledTask{}, nil, err
	}
	schedOps, err := c.scheduleChanges(ctx, item)
	if err != nil {
		return protocol.ScheduledTask{}, nil, err
	}
	ops := make([]protocol.PluginTransactionOperation, 0, 1+len(schedOps))
	ops = append(ops, protocol.PluginTransactionOperation{
		Kind: "write",
		Data: protocol.PluginTransactionOperationWritePayload{
			Key:              k,
			Value:            encoded,
			ExpectedRevision: expectedRev,
			Scope:            protocol.PluginStorageScopeNode,
		},
	})
	ops = append(ops, schedOps...)
	return item, ops, nil
}

func (c storageClient) removeOperations(ctx context.Context, id string, rev uint64, expectedVal *protocol.ScheduledTask, scopeProject *protocol.ProjectID, scoped bool) ([]protocol.PluginTransactionOperation, error) {
	old, err := c.read(ctx, id)
	if err != nil {
		return nil, err
	}
	if !old.present {
		return nil, fault(protocol.ErrorCodeNotFound, "Task is unavailable")
	}
	if scoped && !sameProject(old.value.Project, scopeProject) {
		return nil, fault(protocol.ErrorCodePermissionDenied, "Task belongs to another project")
	}
	expectedRev, err := checkBase(rev, expectedVal, old.present, old.revision, old.value)
	if err != nil {
		return nil, err
	}
	target, err := c.executionNode(ctx, old.value)
	if err != nil {
		return nil, err
	}
	if !sameNode(target, c.host.LocalNodeID()) {
		return nil, fault(protocol.ErrorCodeConflict, "Task remains on its original execution Node")
	}
	k, err := taskKey(id)
	if err != nil {
		return nil, err
	}
	remOps, err := c.scheduleRemovals(ctx, id)
	if err != nil {
		return nil, err
	}
	ops := make([]protocol.PluginTransactionOperation, 0, 1+len(remOps))
	ops = append(ops, protocol.PluginTransactionOperation{
		Kind: "remove",
		Data: protocol.PluginTransactionOperationRemovePayload{
			Key:              k,
			ExpectedRevision: expectedRev,
			Scope:            protocol.PluginStorageScopeNode,
		},
	})
	ops = append(ops, remOps...)
	return ops, nil
}

func extractWrittenEntry(outputs []protocol.Output, id string) (protocol.PluginStorageEntry, error) {
	wantKey, err := taskKey(id)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	for _, out := range outputs {
		if out.Kind != "plugin_value" {
			continue
		}
		var entry protocol.PluginStorageEntry
		switch v := out.Data.(type) {
		case protocol.PluginStorageEntry:
			entry = v
		case *protocol.PluginStorageEntry:
			if v != nil {
				entry = *v
			}
		default:
			raw, err := json.Marshal(out.Data)
			if err != nil || json.Unmarshal(raw, &entry) != nil {
				continue
			}
		}
		if entry.Key == wantKey {
			return entry, nil
		}
	}
	return protocol.PluginStorageEntry{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task outcome is unavailable")
}

func extractEnqueuedJob(outputs []protocol.Output) (protocol.DispatchJob, error) {
	if len(outputs) < 3 || outputs[2].Kind != "dispatch" {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task run outcome is unavailable")
	}
	var disp protocol.DispatchOutput
	switch v := outputs[2].Data.(type) {
	case protocol.DispatchOutput:
		disp = v
	case *protocol.DispatchOutput:
		if v != nil {
			disp = *v
		}
	default:
		raw, err := json.Marshal(outputs[2].Data)
		if err != nil || json.Unmarshal(raw, &disp) != nil {
			return protocol.DispatchJob{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task run outcome is unavailable")
		}
	}
	if disp.Kind != "job" {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task run outcome is unavailable")
	}
	job, ok := unwrapDispatchJob(disp.Data)
	if !ok {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task run outcome is unavailable")
	}
	return job, nil
}

func extractHistoryTurn(outcome protocol.RequestOutcome) (*protocol.SessionID, *protocol.TurnID) {
	if outcome.Kind != "completed" {
		return nil, nil
	}
	var res protocol.Result[protocol.Output, protocol.Fault]
	switch v := outcome.Data.(type) {
	case protocol.Result[protocol.Output, protocol.Fault]:
		res = v
	case *protocol.Result[protocol.Output, protocol.Fault]:
		if v != nil {
			res = *v
		}
	default:
		raw, err := json.Marshal(outcome.Data)
		if err != nil || json.Unmarshal(raw, &res) != nil {
			return nil, nil
		}
	}
	if res.Ok == nil || res.Ok.Kind != "queued_turn" {
		return nil, nil
	}
	var turn protocol.QueuedTurn
	switch v := res.Ok.Data.(type) {
	case protocol.QueuedTurn:
		turn = v
	case *protocol.QueuedTurn:
		if v != nil {
			turn = *v
		} else {
			return nil, nil
		}
	default:
		raw, err := json.Marshal(res.Ok.Data)
		if err != nil || json.Unmarshal(raw, &turn) != nil {
			return nil, nil
		}
	}
	sessionID := turn.Session
	turnID := turn.ID
	return &sessionID, &turnID
}

func extractJobName(job protocol.DispatchJob) string {
	var payload struct {
		Name *string `json:"name"`
	}
	if len(job.Event.Payload) > 0 && json.Unmarshal(job.Event.Payload, &payload) == nil && payload.Name != nil {
		return *payload.Name
	}
	return job.Handler
}

func unwrapScheduledTaskEntry(data any) (protocol.ScheduledTaskEntry, bool) {
	switch v := data.(type) {
	case protocol.ScheduledTaskEntry:
		return v, true
	case *protocol.ScheduledTaskEntry:
		if v != nil {
			return *v, true
		}
	}
	raw, err := json.Marshal(data)
	if err != nil {
		return protocol.ScheduledTaskEntry{}, false
	}
	var entry protocol.ScheduledTaskEntry
	if json.Unmarshal(raw, &entry) != nil {
		return protocol.ScheduledTaskEntry{}, false
	}
	return entry, true
}

func unwrapDispatchJob(data any) (protocol.DispatchJob, bool) {
	switch v := data.(type) {
	case protocol.DispatchJob:
		return v, true
	case *protocol.DispatchJob:
		if v != nil {
			return *v, true
		}
	}
	raw, err := json.Marshal(data)
	if err != nil {
		return protocol.DispatchJob{}, false
	}
	var job protocol.DispatchJob
	if json.Unmarshal(raw, &job) != nil {
		return protocol.DispatchJob{}, false
	}
	return job, true
}

func unwrapScheduledTaskHistory(data any) (protocol.ScheduledTaskHistory, bool) {
	switch v := data.(type) {
	case protocol.ScheduledTaskHistory:
		return v, true
	case *protocol.ScheduledTaskHistory:
		if v != nil {
			return *v, true
		}
	}
	raw, err := json.Marshal(data)
	if err != nil {
		return protocol.ScheduledTaskHistory{}, false
	}
	var history protocol.ScheduledTaskHistory
	if json.Unmarshal(raw, &history) != nil {
		return protocol.ScheduledTaskHistory{}, false
	}
	return history, true
}

func unwrapScheduledTaskStopped(data any) (protocol.ScheduledTaskStopped, bool) {
	switch v := data.(type) {
	case protocol.ScheduledTaskStopped:
		return v, true
	case *protocol.ScheduledTaskStopped:
		if v != nil {
			return *v, true
		}
	}
	raw, err := json.Marshal(data)
	if err != nil {
		return protocol.ScheduledTaskStopped{}, false
	}
	var stopped protocol.ScheduledTaskStopped
	if json.Unmarshal(raw, &stopped) != nil {
		return protocol.ScheduledTaskStopped{}, false
	}
	return stopped, true
}
