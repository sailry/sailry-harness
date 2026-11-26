package scheduledtasksnode

import (
	"context"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (module) HandlesCommand(kind string) bool {
	switch kind {
	case "list_scheduled_tasks",
		"read_scheduled_task",
		"save_scheduled_task",
		"remove_scheduled_task",
		"run_scheduled_task",
		"read_scheduled_task_runtime",
		"read_scheduled_task_history",
		"cancel_scheduled_task_job",
		"stop_scheduled_task_job":
		return true
	}
	return false
}

func (module) ExecuteCommand(ctx context.Context, host moduleapi.Host, _ protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	client := storageClient{host: host}
	switch request.Command.Kind {
	case "list_scheduled_tasks":
		input, _ := request.Command.Data.(*protocol.CommandListScheduledTasksPayload)
		var after *string
		if input != nil {
			after = input.After
		}
		page, err := client.listing(ctx, after, nil, false)
		return protocol.Output{Kind: "scheduled_task_page", Data: page}, nil, err

	case "read_scheduled_task":
		input, ok := request.Command.Data.(*protocol.CommandReadScheduledTaskPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task read request")
		}
		rec, err := client.read(ctx, input.ID)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if !rec.present {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "Task is unavailable")
		}
		target, err := client.executionNode(ctx, rec.value)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if !sameNode(target, host.LocalNodeID()) {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "Task belongs to another execution Node")
		}
		val := rec.value
		val.ExecutionNode = &target
		rev := rec.revision
		return protocol.Output{Kind: "scheduled_task_entry", Data: protocol.ScheduledTaskEntry{Task: val, Revision: &rev}}, nil, nil

	case "save_scheduled_task":
		input, ok := request.Command.Data.(*protocol.CommandSaveScheduledTaskPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task save request")
		}
		out, err := client.handleSave(ctx, *input)
		return protocol.Output{Kind: "scheduled_task_entry", Data: out}, nil, err

	case "remove_scheduled_task":
		input, ok := request.Command.Data.(*protocol.CommandRemoveScheduledTaskPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task removal request")
		}
		out, err := client.handleRemove(ctx, *input)
		return protocol.Output{Kind: "scheduled_task_removed", Data: out}, nil, err

	case "run_scheduled_task":
		input, ok := request.Command.Data.(*protocol.CommandRunScheduledTaskPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task run request")
		}
		out, err := client.handleRun(ctx, *input)
		return protocol.Output{Kind: "scheduled_task_job", Data: out}, nil, err

	case "read_scheduled_task_runtime":
		out, err := client.handleRuntime(ctx)
		return protocol.Output{Kind: "scheduled_task_runtime", Data: out}, nil, err

	case "read_scheduled_task_history":
		input, _ := request.Command.Data.(*protocol.CommandReadScheduledTaskHistoryPayload)
		var payload protocol.CommandReadScheduledTaskHistoryPayload
		if input != nil {
			payload = *input
		}
		out, err := client.handleHistory(ctx, payload)
		return protocol.Output{Kind: "scheduled_task_history", Data: out}, nil, err

	case "cancel_scheduled_task_job":
		input, ok := request.Command.Data.(*protocol.CommandCancelScheduledTaskJobPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task cancellation request")
		}
		out, err := client.handleCancel(ctx, *input)
		return protocol.Output{Kind: "scheduled_task_job", Data: out}, nil, err

	case "stop_scheduled_task_job":
		input, ok := request.Command.Data.(*protocol.CommandStopScheduledTaskJobPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task stop request")
		}
		out, err := client.handleStop(ctx, *input)
		return protocol.Output{Kind: "scheduled_task_stopped", Data: out}, nil, err

	default:
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "unsupported scheduled task command")
	}
}

func (c storageClient) handleSave(ctx context.Context, input protocol.CommandSaveScheduledTaskPayload) (protocol.ScheduledTaskEntry, error) {
	local := c.host.LocalNodeID()
	var old *storedRecord
	if input.Task.ID != "" {
		rec, err := c.read(ctx, input.Task.ID)
		if err != nil {
			return protocol.ScheduledTaskEntry{}, err
		}
		if rec.present {
			old = &rec
			target, err := c.executionNode(ctx, rec.value)
			if err != nil {
				return protocol.ScheduledTaskEntry{}, err
			}
			if input.Task.ExecutionNode != nil && !sameNode(*input.Task.ExecutionNode, target) {
				return protocol.ScheduledTaskEntry{}, fault(protocol.ErrorCodeConflict, "Task remains on its original execution Node")
			}
			input.Task.ExecutionNode = &target
		}
	}
	target := local
	if input.Task.ExecutionNode != nil {
		if err := validateNodeID(input.Task.ExecutionNode); err != nil {
			return protocol.ScheduledTaskEntry{}, err
		}
		target = *input.Task.ExecutionNode
	}
	if !sameNode(target, local) {
		forwarded := input
		if old != nil && forwarded.ExpectedValue == nil {
			cp := old.value
			forwarded.ExpectedValue = &cp
		}
		out, err := c.host.ForwardModuleCommand(ctx, target, protocol.Command{
			Kind: "save_scheduled_task",
			Data: &forwarded,
		})
		if err != nil {
			return protocol.ScheduledTaskEntry{}, err
		}
		entry, ok := unwrapScheduledTaskEntry(out.Data)
		if !ok {
			return protocol.ScheduledTaskEntry{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task outcome is unavailable")
		}
		return entry, nil
	}
	item, ops, err := c.saveOperations(ctx, input.Task, input.Revision, input.ExpectedValue, nil, false)
	if err != nil {
		return protocol.ScheduledTaskEntry{}, err
	}
	outputs, err := c.transaction(ctx, ops)
	if err != nil {
		return protocol.ScheduledTaskEntry{}, err
	}
	written, err := extractWrittenEntry(outputs, item.ID)
	if err != nil || !written.Present {
		return protocol.ScheduledTaskEntry{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task outcome is unavailable")
	}
	rev := written.Revision
	return protocol.ScheduledTaskEntry{Task: item, Revision: &rev}, nil
}

func (c storageClient) handleRemove(ctx context.Context, input protocol.CommandRemoveScheduledTaskPayload) (protocol.ScheduledTaskRemoved, error) {
	local := c.host.LocalNodeID()
	old, err := c.read(ctx, input.ID)
	if err != nil {
		return protocol.ScheduledTaskRemoved{}, err
	}
	if old.present {
		target, err := c.executionNode(ctx, old.value)
		if err != nil {
			return protocol.ScheduledTaskRemoved{}, err
		}
		if !sameNode(target, local) {
			forwarded := input
			if forwarded.ExpectedValue == nil {
				cp := old.value
				forwarded.ExpectedValue = &cp
			}
			out, err := c.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "remove_scheduled_task",
				Data: &forwarded,
			})
			if err != nil {
				return protocol.ScheduledTaskRemoved{}, err
			}
			switch v := out.Data.(type) {
			case protocol.ScheduledTaskRemoved:
				return v, nil
			case *protocol.ScheduledTaskRemoved:
				if v != nil {
					return *v, nil
				}
			}
			return protocol.ScheduledTaskRemoved{ID: input.ID, Removed: true}, nil
		}
	}
	ops, err := c.removeOperations(ctx, input.ID, input.Revision, input.ExpectedValue, nil, false)
	if err != nil {
		return protocol.ScheduledTaskRemoved{}, err
	}
	if _, err := c.transaction(ctx, ops); err != nil {
		return protocol.ScheduledTaskRemoved{}, err
	}
	return protocol.ScheduledTaskRemoved{ID: input.ID, Removed: true}, nil
}

func (c storageClient) handleRun(ctx context.Context, input protocol.CommandRunScheduledTaskPayload) (protocol.DispatchJob, error) {
	k, err := taskKey(input.ID)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	rec, err := c.read(ctx, input.ID)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	if !rec.present {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeNotFound, "Task is unavailable")
	}
	target, err := c.executionNode(ctx, rec.value)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	local := c.host.LocalNodeID()
	if !sameNode(target, local) {
		forwarded := input
		if forwarded.Revision == nil {
			rev := rec.revision
			forwarded.Revision = &rev
		}
		if forwarded.ExpectedValue == nil {
			cp := rec.value
			forwarded.ExpectedValue = &cp
		}
		out, err := c.host.ForwardModuleCommand(ctx, target, protocol.Command{
			Kind: "run_scheduled_task",
			Data: &forwarded,
		})
		if err != nil {
			return protocol.DispatchJob{}, err
		}
		job, ok := unwrapDispatchJob(out.Data)
		if !ok {
			return protocol.DispatchJob{}, fault(protocol.ErrorCodeOutcomeUnknown, "Task run outcome is unavailable")
		}
		return job, nil
	}
	if input.Revision != nil || input.ExpectedValue != nil {
		var rev uint64
		if input.Revision != nil {
			rev = *input.Revision
		}
		if _, err := checkBase(rev, input.ExpectedValue, rec.present, rec.revision, rec.value); err != nil {
			return protocol.DispatchJob{}, err
		}
	}
	// The KV fence also catches source snapshots that changed the local view
	// without updating this execution Node's handler.
	handlers, err := c.listHandlers(ctx)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	var handler *protocol.DispatchHandler
	for i := range handlers {
		if handlers[i].Name == input.ID {
			handler = &handlers[i]
			break
		}
	}
	old, err := c.read(ctx, input.ID)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	if !old.present {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeNotFound, "Task is unavailable")
	}
	if input.Revision != nil || input.ExpectedValue != nil {
		var rev uint64
		if input.Revision != nil {
			rev = *input.Revision
		}
		if _, err := checkBase(rev, input.ExpectedValue, old.present, old.revision, old.value); err != nil {
			return protocol.DispatchJob{}, err
		}
	}
	resolvedCfg, err := c.resolveConfig(ctx, old.value.Config, old.value.Team)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	if handler == nil {
		return protocol.DispatchJob{}, fault(protocol.ErrorCodeNotFound, "Task handler is unavailable")
	}
	current := old.value
	current.Config = resolvedCfg
	fenceValue, err := marshalRaw(old.value)
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	enqueuePayload, err := marshalRaw(map[string]string{"name": old.value.Name})
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	outputs, err := c.transaction(ctx, []protocol.PluginTransactionOperation{
		{
			Kind: "write",
			Data: protocol.PluginTransactionOperationWritePayload{
				Key:              k,
				Value:            fenceValue,
				ExpectedRevision: old.revision,
				Scope:            protocol.PluginStorageScopeNode,
			},
		},
		c.handlerOperation(current, handler.Revision),
		{
			Kind: "dispatch",
			Data: protocol.DispatchCommand{
				Kind: "enqueue",
				Data: protocol.DispatchCommandEnqueuePayload{
					Handler: input.ID,
					Payload: enqueuePayload,
				},
			},
		},
	})
	if err != nil {
		return protocol.DispatchJob{}, err
	}
	return extractEnqueuedJob(outputs)
}

func (c storageClient) handleRuntime(ctx context.Context) (protocol.ScheduledTaskRuntime, error) {
	status := map[string]protocol.ScheduledTaskRunState{}
	var before *uint64
	for {
		page, err := c.listJobs(ctx, before, 100)
		if err != nil {
			return protocol.ScheduledTaskRuntime{}, err
		}
		for _, job := range page.Jobs {
			known, exists := status[job.Handler]
			if !exists || job.Status == protocol.DispatchStatusRunning ||
				(job.Status == protocol.DispatchStatusQueued && known.Status != protocol.DispatchStatusRunning) {
				status[job.Handler] = protocol.ScheduledTaskRunState{
					Status:    job.Status,
					StartedMs: job.StartedMs,
				}
			}
		}
		if page.NextBefore == nil {
			break
		}
		before = page.NextBefore
	}
	schedules, err := c.listSchedules(ctx)
	if err != nil {
		return protocol.ScheduledTaskRuntime{}, err
	}
	return protocol.ScheduledTaskRuntime{
		Status:    status,
		Schedules: schedules,
	}, nil
}

func (c storageClient) handleHistory(ctx context.Context, input protocol.CommandReadScheduledTaskHistoryPayload) (protocol.ScheduledTaskHistory, error) {
	limit := input.Limit
	if limit == 0 {
		limit = defaultHistoryLimit
	}
	local := c.host.LocalNodeID()
	if input.ExecutionNode != nil {
		if err := validateNodeID(input.ExecutionNode); err != nil {
			return protocol.ScheduledTaskHistory{}, err
		}
		if !sameNode(*input.ExecutionNode, local) {
			out, err := c.host.ForwardModuleCommand(ctx, *input.ExecutionNode, protocol.Command{
				Kind: "read_scheduled_task_history",
				Data: &input,
			})
			if err != nil {
				return protocol.ScheduledTaskHistory{}, err
			}
			history, ok := unwrapScheduledTaskHistory(out.Data)
			if !ok {
				return protocol.ScheduledTaskHistory{Items: []protocol.ScheduledTaskHistoryEntry{}}, nil
			}
			return history, nil
		}
	}
	if input.Handler != nil {
		old, err := c.read(ctx, *input.Handler)
		if err != nil {
			return protocol.ScheduledTaskHistory{}, err
		}
		if old.present {
			target, err := c.executionNode(ctx, old.value)
			if err != nil {
				return protocol.ScheduledTaskHistory{}, err
			}
			if !sameNode(target, local) {
				forwarded := input
				forwarded.ExecutionNode = &target
				out, err := c.host.ForwardModuleCommand(ctx, target, protocol.Command{
					Kind: "read_scheduled_task_history",
					Data: &forwarded,
				})
				if err != nil {
					return protocol.ScheduledTaskHistory{}, err
				}
				history, ok := unwrapScheduledTaskHistory(out.Data)
				if !ok {
					return protocol.ScheduledTaskHistory{Items: []protocol.ScheduledTaskHistoryEntry{}}, nil
				}
				return history, nil
			}
		}
	}
	items := []protocol.ScheduledTaskHistoryEntry{}
	before := input.Before
	for {
		pageLimit := limit
		if input.Handler != nil {
			pageLimit = 100
		}
		page, err := c.listJobs(ctx, before, pageLimit)
		if err != nil {
			return protocol.ScheduledTaskHistory{}, err
		}
		for _, job := range page.Jobs {
			if input.Handler != nil && job.Handler != *input.Handler {
				continue
			}
			receipt, err := c.readJobResult(ctx, job.ID)
			if err != nil {
				return protocol.ScheduledTaskHistory{}, err
			}
			session, turn := extractHistoryTurn(receipt)
			items = append(items, protocol.ScheduledTaskHistoryEntry{
				Job:           job,
				ExecutionNode: local,
				Name:          extractJobName(job),
				Session:       session,
				Turn:          turn,
			})
		}
		before = page.NextBefore
		if input.Handler == nil || len(items) >= int(limit) || before == nil {
			break
		}
	}
	return protocol.ScheduledTaskHistory{Items: items, NextBefore: before}, nil
}

func (c storageClient) handleCancel(ctx context.Context, input protocol.CommandCancelScheduledTaskJobPayload) (protocol.DispatchJob, error) {
	local := c.host.LocalNodeID()
	if input.ExecutionNode != nil {
		if err := validateNodeID(input.ExecutionNode); err != nil {
			return protocol.DispatchJob{}, err
		}
		if !sameNode(*input.ExecutionNode, local) {
			out, err := c.host.ForwardModuleCommand(ctx, *input.ExecutionNode, protocol.Command{
				Kind: "cancel_scheduled_task_job",
				Data: &input,
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
	}
	return c.cancelJob(ctx, input.ID)
}

func (c storageClient) handleStop(ctx context.Context, input protocol.CommandStopScheduledTaskJobPayload) (protocol.ScheduledTaskStopped, error) {
	local := c.host.LocalNodeID()
	if input.ExecutionNode != nil {
		if err := validateNodeID(input.ExecutionNode); err != nil {
			return protocol.ScheduledTaskStopped{}, err
		}
		if !sameNode(*input.ExecutionNode, local) {
			out, err := c.host.ForwardModuleCommand(ctx, *input.ExecutionNode, protocol.Command{
				Kind: "stop_scheduled_task_job",
				Data: &input,
			})
			if err != nil {
				return protocol.ScheduledTaskStopped{}, err
			}
			stopped, ok := unwrapScheduledTaskStopped(out.Data)
			if !ok {
				return protocol.ScheduledTaskStopped{ID: input.ID, Stopped: true}, nil
			}
			return stopped, nil
		}
	}
	if err := c.host.StopDispatchTurn(ctx, storageNamespace, input.ID); err != nil {
		return protocol.ScheduledTaskStopped{}, err
	}
	return protocol.ScheduledTaskStopped{ID: input.ID, Stopped: true}, nil
}
