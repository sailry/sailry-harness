package remindersnode

import (
	"context"
	"encoding/json"
	"errors"
	"strconv"
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
		return v, nil
	case *[]protocol.DispatchSchedule:
		if v != nil {
			return *v, nil
		}
	}
	raw, err := json.Marshal(out.Data)
	if err != nil {
		return nil, err
	}
	var items []protocol.DispatchSchedule
	if err := json.Unmarshal(raw, &items); err != nil {
		return nil, err
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

type storedRecord struct {
	present  bool
	revision uint64
	value    protocol.Reminder
}

func (c storageClient) read(ctx context.Context, id string) (storedRecord, error) {
	k, err := reminderKey(id)
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
	parsed, err := parseStoredReminder(entry.Value, id)
	if err != nil {
		return storedRecord{}, err
	}
	return storedRecord{present: true, revision: entry.Revision, value: parsed}, nil
}

// executionNode resolves the execution Node owning item. Legacy records have no
// execution_node field; only local dispatch records prove local ownership.
func (c storageClient) executionNode(ctx context.Context, item protocol.Reminder) (protocol.NodeID, error) {
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
	return protocol.NodeID{}, fault(protocol.ErrorCodeUnavailable, "Reminder execution Node is unknown")
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

func (c storageClient) visible(ctx context.Context, id string) (*protocol.ReminderEntry, error) {
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
		return &protocol.ReminderEntry{Reminder: rec.value, Revision: &rev}, nil
	}
	out, err := c.host.ForwardModuleCommand(ctx, *rec.value.ExecutionNode, protocol.Command{
		Kind: "read_reminder",
		Data: &protocol.CommandReadReminderPayload{ID: id},
	})
	if err != nil {
		if isMissingOrDeniedFault(err) {
			return nil, nil
		}
		if isOfflineFault(err) {
			return &protocol.ReminderEntry{Reminder: rec.value, Revision: nil, Unavailable: true}, nil
		}
		return nil, err
	}
	entry, ok := unwrapReminderEntry(out.Data)
	if !ok {
		return nil, nil
	}
	return &entry, nil
}

func (c storageClient) page(ctx context.Context, after *string, completed *bool, scopeProject *protocol.ProjectID, scoped bool) (protocol.ReminderPage, error) {
	var cursor *string
	if after != nil && *after != "" {
		if !strings.HasPrefix(*after, keyPrefix) {
			return protocol.ReminderPage{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder ID")
		}
		if _, err := reminderKey(strings.TrimPrefix(*after, keyPrefix)); err != nil {
			return protocol.ReminderPage{}, err
		}
		cp := *after
		cursor = &cp
	}
	items := []protocol.ReminderEntry{}
	scanned := 0
	size := 0
	for {
		batch, err := c.listKeys(ctx, keyPrefix, cursor, maxPageItems)
		if err != nil {
			return protocol.ReminderPage{}, err
		}
		for idx, name := range batch.Keys {
			id := strings.TrimPrefix(name, keyPrefix)
			entry, err := c.visible(ctx, id)
			if err != nil {
				return protocol.ReminderPage{}, err
			}
			if entry != nil &&
				(!scoped || sameProject(entry.Reminder.Project, scopeProject)) &&
				(completed == nil || entry.Reminder.Completed == *completed) {
				encoded, err := json.Marshal(entry)
				if err != nil {
					return protocol.ReminderPage{}, err
				}
				if len(items) > 0 && size+len(encoded) > maxPageBytes {
					return protocol.ReminderPage{Items: items, After: cursor}, nil
				}
				items = append(items, *entry)
				size += len(encoded)
			}
			cp := name
			cursor = &cp
			scanned++
			if len(items) == maxPageItems || scanned == maxScanKeys {
				last := idx == len(batch.Keys)-1 && batch.After == nil
				if last {
					return protocol.ReminderPage{Items: items, After: nil}, nil
				}
				return protocol.ReminderPage{Items: items, After: cursor}, nil
			}
		}
		if batch.After == nil {
			break
		}
		cursor = batch.After
	}
	return protocol.ReminderPage{Items: items, After: nil}, nil
}

type deliveryPayload struct {
	ID            string             `json:"id"`
	Revision      string             `json:"revision"`
	ExpectedValue *protocol.Reminder `json:"expected_value,omitempty"`
}

func (c storageClient) scheduleChanges(ctx context.Context, item protocol.Reminder, nextRev uint64) ([]protocol.PluginTransactionOperation, error) {
	schedules, err := c.listSchedules(ctx)
	if err != nil {
		return nil, err
	}
	handlers, err := c.listHandlers(ctx)
	if err != nil {
		return nil, err
	}
	var schedule *protocol.DispatchSchedule
	for i := range schedules {
		if schedules[i].ID.String() == item.ID {
			schedule = &schedules[i]
			break
		}
	}
	var handler *protocol.DispatchHandler
	for i := range handlers {
		if handlers[i].Name == item.ID {
			handler = &handlers[i]
			break
		}
	}
	scheduleID, err := protocol.ParseScheduleID(item.ID)
	if err != nil {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder ID")
	}
	enabled := item.DueMs != nil && !item.Completed && item.NotifiedMs == nil
	ops := make([]protocol.PluginTransactionOperation, 0, 3)
	// When a one-shot schedule has already fired (NextMs == nil), remove and
	// recreate it at revision 0 so Node dispatch computes a fresh NextMs.
	if enabled && schedule != nil && schedule.NextMs == nil {
		ops = append(ops, protocol.PluginTransactionOperation{
			Kind: "dispatch",
			Data: protocol.DispatchCommand{
				Kind: "remove_schedule",
				Data: protocol.DispatchCommandRemoveSchedulePayload{
					ID:               scheduleID,
					ExpectedRevision: schedule.Revision,
				},
			},
		})
		schedule = nil
	}
	var handlerRev uint64
	if handler != nil {
		handlerRev = handler.Revision
	}
	var scopeWorktree *protocol.WorktreeID
	var scopeSession *protocol.SessionID
	if c.inv != nil {
		w := c.inv.Run.Worktree
		s := c.inv.Session.ID
		scopeWorktree = &w
		scopeSession = &s
	}
	ops = append(ops, protocol.PluginTransactionOperation{
		Kind: "dispatch",
		Data: protocol.DispatchCommand{
			Kind: "save_handler",
			Data: protocol.DispatchHandler{
				Name:     item.ID,
				Revision: handlerRev,
				Enabled:  true,
				Source:   protocol.DispatchSource{Package: storageNamespace, Topic: item.ID},
				Queue:    storageNamespace,
				Callback: protocol.DispatchCallback{
					Scope:      protocol.DispatchScope{Worktree: scopeWorktree, Session: scopeSession},
					Completion: protocol.DispatchCompletionCommand,
					Command: &protocol.Command{
						Kind: "deliver_reminder",
						Data: &protocol.CommandDeliverReminderPayload{Input: json.RawMessage("null")},
					},
					Bindings: map[string]string{"/data/input": "/payload"},
				},
			},
		},
	})
	var scheduleRev uint64
	if schedule != nil {
		scheduleRev = schedule.Revision
	}
	var dueMs int64
	if item.DueMs != nil {
		dueMs = *item.DueMs
	}
	payloadRaw, err := marshalRaw(deliveryPayload{
		ID:            item.ID,
		Revision:      strconv.FormatUint(nextRev, 10),
		ExpectedValue: &item,
	})
	if err != nil {
		return nil, err
	}
	ops = append(ops, protocol.PluginTransactionOperation{
		Kind: "dispatch",
		Data: protocol.DispatchCommand{
			Kind: "save_schedule",
			Data: protocol.DispatchSchedule{
				ID:       scheduleID,
				Revision: scheduleRev,
				Enabled:  enabled,
				Topic:    item.ID,
				Payload:  payloadRaw,
				Timing: protocol.DispatchTiming{
					Kind: "once",
					Data: protocol.DispatchTimingOncePayload{AtMs: dueMs},
				},
				NextMs: nil,
			},
		},
	})
	return ops, nil
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
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder ID")
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
					},
				},
			})
			break
		}
	}
	return ops, nil
}

func (c storageClient) saveOperations(ctx context.Context, input protocol.Reminder, rev uint64, expectedVal *protocol.Reminder, scopeProject *protocol.ProjectID, scoped bool) (protocol.Reminder, []protocol.PluginTransactionOperation, error) {
	local := c.host.LocalNodeID()
	item, err := normalizeDraft(input, local, true)
	if err != nil {
		return protocol.Reminder{}, nil, err
	}
	if scoped && !sameProject(item.Project, scopeProject) {
		return protocol.Reminder{}, nil, fault(protocol.ErrorCodePermissionDenied, "Reminder belongs to another project")
	}
	old, err := c.read(ctx, item.ID)
	if err != nil {
		return protocol.Reminder{}, nil, err
	}
	if old.present && scoped && !sameProject(old.value.Project, scopeProject) {
		return protocol.Reminder{}, nil, fault(protocol.ErrorCodePermissionDenied, "Reminder belongs to another project")
	}
	expectedRev, err := checkBase(rev, expectedVal, old.present, old.revision, old.value)
	if err != nil {
		return protocol.Reminder{}, nil, err
	}
	var target protocol.NodeID
	if old.present {
		target, err = c.executionNode(ctx, old.value)
		if err != nil {
			return protocol.Reminder{}, nil, err
		}
	} else {
		target = *item.ExecutionNode
	}
	if !sameNode(target, local) || item.ExecutionNode == nil || !sameNode(*item.ExecutionNode, target) {
		return protocol.Reminder{}, nil, fault(protocol.ErrorCodeConflict, "Reminder remains on its original execution Node")
	}
	item.ExecutionNode = &target
	if !scoped && item.Project != nil {
		catalog, err := c.host.ReadProjectCatalog(ctx)
		if err != nil {
			return protocol.Reminder{}, nil, err
		}
		found := false
		for _, p := range catalog.Projects {
			if p.ID == *item.Project {
				found = true
				break
			}
		}
		if !found {
			return protocol.Reminder{}, nil, fault(protocol.ErrorCodeNotFound, "Reminder project is unavailable")
		}
	}
	if old.present && sameInt64Ptr(old.value.DueMs, item.DueMs) {
		item.NotifiedMs = old.value.NotifiedMs
	} else {
		item.NotifiedMs = nil
	}
	k, err := reminderKey(item.ID)
	if err != nil {
		return protocol.Reminder{}, nil, err
	}
	encoded, err := marshalRaw(item)
	if err != nil {
		return protocol.Reminder{}, nil, err
	}
	schedOps, err := c.scheduleChanges(ctx, item, expectedRev+1)
	if err != nil {
		return protocol.Reminder{}, nil, err
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

func (c storageClient) removeOperations(ctx context.Context, id string, rev uint64, expectedVal *protocol.Reminder, scopeProject *protocol.ProjectID, scoped bool) ([]protocol.PluginTransactionOperation, error) {
	old, err := c.read(ctx, id)
	if err != nil {
		return nil, err
	}
	if !old.present {
		return nil, fault(protocol.ErrorCodeNotFound, "Reminder is unavailable")
	}
	if scoped && !sameProject(old.value.Project, scopeProject) {
		return nil, fault(protocol.ErrorCodePermissionDenied, "Reminder belongs to another project")
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
		return nil, fault(protocol.ErrorCodeConflict, "Reminder remains on its original execution Node")
	}
	k, err := reminderKey(id)
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
	wantKey, err := reminderKey(id)
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
	return protocol.PluginStorageEntry{}, fault(protocol.ErrorCodeOutcomeUnknown, "Reminder outcome is unavailable")
}

func receiptSkipped(outcome protocol.RequestOutcome) bool {
	if outcome.Kind != "completed" {
		return false
	}
	raw, err := json.Marshal(outcome.Data)
	if err != nil {
		return false
	}
	var env struct {
		Ok *struct {
			Kind string          `json:"kind"`
			Data json.RawMessage `json:"data"`
		} `json:"Ok"`
	}
	if json.Unmarshal(raw, &env) != nil || env.Ok == nil {
		return false
	}
	var body struct {
		Skipped bool `json:"skipped"`
	}
	return json.Unmarshal(env.Ok.Data, &body) == nil && body.Skipped
}

func unwrapReminderEntry(data any) (protocol.ReminderEntry, bool) {
	switch v := data.(type) {
	case protocol.ReminderEntry:
		return v, true
	case *protocol.ReminderEntry:
		if v != nil {
			return *v, true
		}
	}
	raw, err := json.Marshal(data)
	if err != nil {
		return protocol.ReminderEntry{}, false
	}
	var entry protocol.ReminderEntry
	if json.Unmarshal(raw, &entry) != nil {
		return protocol.ReminderEntry{}, false
	}
	return entry, true
}
