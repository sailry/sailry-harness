package remindersnode

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var reminderNotificationPresentation = &protocol.NotificationPresentation{
	Heading:    "Reminder",
	Locales:    map[string]string{"zh-CN": "提醒"},
	Persistent: true,
}

func (module) HandlesCommand(kind string) bool {
	switch kind {
	case "list_reminders", "read_reminder", "save_reminder", "remove_reminder", "read_reminder_history", "deliver_reminder":
		return true
	}
	return false
}

func (module) ExecuteCommand(ctx context.Context, host moduleapi.Host, _ protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	client := storageClient{host: host}
	switch request.Command.Kind {
	case "list_reminders":
		input, _ := request.Command.Data.(*protocol.CommandListRemindersPayload)
		var after *string
		var completed *bool
		if input != nil {
			after, completed = input.After, input.Completed
		}
		page, err := client.page(ctx, after, completed, nil, false)
		return protocol.Output{Kind: "reminder_page", Data: page}, nil, err

	case "read_reminder":
		input, ok := request.Command.Data.(*protocol.CommandReadReminderPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder read request")
		}
		rec, err := client.read(ctx, input.ID)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if !rec.present {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "Reminder is unavailable")
		}
		target, err := client.executionNode(ctx, rec.value)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if !sameNode(target, host.LocalNodeID()) {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "Reminder belongs to another execution Node")
		}
		val := rec.value
		val.ExecutionNode = &target
		rev := rec.revision
		return protocol.Output{Kind: "reminder_entry", Data: protocol.ReminderEntry{Reminder: val, Revision: &rev}}, nil, nil

	case "save_reminder":
		input, ok := request.Command.Data.(*protocol.CommandSaveReminderPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder save request")
		}
		out, err := client.handleSave(ctx, *input)
		return protocol.Output{Kind: "reminder_entry", Data: out}, nil, err

	case "remove_reminder":
		input, ok := request.Command.Data.(*protocol.CommandRemoveReminderPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder removal request")
		}
		out, err := client.handleRemove(ctx, *input)
		return protocol.Output{Kind: "reminder_removed", Data: out}, nil, err

	case "read_reminder_history":
		input, ok := request.Command.Data.(*protocol.CommandReadReminderHistoryPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder history request")
		}
		out, err := client.handleHistory(ctx, input.Handler)
		return protocol.Output{Kind: "reminder_history", Data: out}, nil, err

	case "deliver_reminder":
		input, ok := request.Command.Data.(*protocol.CommandDeliverReminderPayload)
		if !ok || input == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder delivery request")
		}
		out, err := client.handleDeliver(ctx, input.Input)
		return protocol.Output{Kind: "reminder_delivery", Data: out}, nil, err

	default:
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "unsupported reminder command")
	}
}

func (c storageClient) handleSave(ctx context.Context, input protocol.CommandSaveReminderPayload) (protocol.ReminderEntry, error) {
	local := c.host.LocalNodeID()
	var old *storedRecord
	if input.Reminder.ID != "" {
		rec, err := c.read(ctx, input.Reminder.ID)
		if err != nil {
			return protocol.ReminderEntry{}, err
		}
		if rec.present {
			old = &rec
			target, err := c.executionNode(ctx, rec.value)
			if err != nil {
				return protocol.ReminderEntry{}, err
			}
			if input.Reminder.ExecutionNode != nil && !sameNode(*input.Reminder.ExecutionNode, target) {
				return protocol.ReminderEntry{}, fault(protocol.ErrorCodeConflict, "Reminder remains on its original execution Node")
			}
			input.Reminder.ExecutionNode = &target
		}
	}
	target := local
	if input.Reminder.ExecutionNode != nil {
		if err := validateNodeID(input.Reminder.ExecutionNode); err != nil {
			return protocol.ReminderEntry{}, err
		}
		target = *input.Reminder.ExecutionNode
	}
	if !sameNode(target, local) {
		forwarded := input
		if old != nil && forwarded.ExpectedValue == nil {
			cp := old.value
			forwarded.ExpectedValue = &cp
		}
		out, err := c.host.ForwardModuleCommand(ctx, target, protocol.Command{
			Kind: "save_reminder",
			Data: &forwarded,
		})
		if err != nil {
			return protocol.ReminderEntry{}, err
		}
		entry, ok := unwrapReminderEntry(out.Data)
		if !ok {
			return protocol.ReminderEntry{}, fault(protocol.ErrorCodeOutcomeUnknown, "Reminder outcome is unavailable")
		}
		return entry, nil
	}
	item, ops, err := c.saveOperations(ctx, input.Reminder, input.Revision, input.ExpectedValue, nil, false)
	if err != nil {
		return protocol.ReminderEntry{}, err
	}
	outputs, err := c.transaction(ctx, ops)
	if err != nil {
		return protocol.ReminderEntry{}, err
	}
	written, err := extractWrittenEntry(outputs, item.ID)
	if err != nil || !written.Present {
		return protocol.ReminderEntry{}, fault(protocol.ErrorCodeOutcomeUnknown, "Reminder outcome is unavailable")
	}
	rev := written.Revision
	return protocol.ReminderEntry{Reminder: item, Revision: &rev}, nil
}

func (c storageClient) handleRemove(ctx context.Context, input protocol.CommandRemoveReminderPayload) (protocol.ReminderRemoved, error) {
	local := c.host.LocalNodeID()
	old, err := c.read(ctx, input.ID)
	if err != nil {
		return protocol.ReminderRemoved{}, err
	}
	if old.present {
		target, err := c.executionNode(ctx, old.value)
		if err != nil {
			return protocol.ReminderRemoved{}, err
		}
		if !sameNode(target, local) {
			forwarded := input
			if forwarded.ExpectedValue == nil {
				cp := old.value
				forwarded.ExpectedValue = &cp
			}
			out, err := c.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "remove_reminder",
				Data: &forwarded,
			})
			if err != nil {
				return protocol.ReminderRemoved{}, err
			}
			switch v := out.Data.(type) {
			case protocol.ReminderRemoved:
				return v, nil
			case *protocol.ReminderRemoved:
				if v != nil {
					return *v, nil
				}
			}
			return protocol.ReminderRemoved{ID: input.ID, Removed: true}, nil
		}
	}
	ops, err := c.removeOperations(ctx, input.ID, input.Revision, input.ExpectedValue, nil, false)
	if err != nil {
		return protocol.ReminderRemoved{}, err
	}
	if _, err := c.transaction(ctx, ops); err != nil {
		return protocol.ReminderRemoved{}, err
	}
	return protocol.ReminderRemoved{ID: input.ID, Removed: true}, nil
}

func (c storageClient) handleHistory(ctx context.Context, handler string) (protocol.ReminderHistory, error) {
	if _, err := reminderKey(handler); err != nil {
		return protocol.ReminderHistory{}, err
	}
	rec, err := c.read(ctx, handler)
	if err != nil {
		return protocol.ReminderHistory{}, err
	}
	local := c.host.LocalNodeID()
	if rec.present {
		target, err := c.executionNode(ctx, rec.value)
		if err != nil {
			return protocol.ReminderHistory{}, err
		}
		if !sameNode(target, local) {
			out, err := c.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "read_reminder_history",
				Data: &protocol.CommandReadReminderHistoryPayload{Handler: handler},
			})
			if err != nil {
				return protocol.ReminderHistory{}, err
			}
			switch v := out.Data.(type) {
			case protocol.ReminderHistory:
				return v, nil
			case *protocol.ReminderHistory:
				if v != nil {
					return *v, nil
				}
			}
			return protocol.ReminderHistory{Items: []protocol.DispatchJob{}}, nil
		}
	}
	items := []protocol.DispatchJob{}
	var before *uint64
	for {
		page, err := c.listJobs(ctx, before, 100)
		if err != nil {
			return protocol.ReminderHistory{}, err
		}
		for _, job := range page.Jobs {
			if job.Handler != handler {
				continue
			}
			if job.Status == protocol.DispatchStatusCompleted {
				receipt, err := c.readJobResult(ctx, job.ID)
				if err != nil {
					return protocol.ReminderHistory{}, err
				}
				if receiptSkipped(receipt) {
					continue
				}
			}
			items = append(items, job)
		}
		if page.NextBefore == nil {
			break
		}
		before = page.NextBefore
	}
	return protocol.ReminderHistory{Items: items}, nil
}

func (c storageClient) handleDeliver(ctx context.Context, raw json.RawMessage) (protocol.ReminderDelivery, error) {
	var payload struct {
		ID            string             `json:"id"`
		Revision      json.RawMessage    `json:"revision"`
		ExpectedValue *protocol.Reminder `json:"expected_value"`
	}
	if err := json.Unmarshal(raw, &payload); err != nil {
		return protocol.ReminderDelivery{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder delivery payload")
	}
	var expectedRev uint64
	if len(payload.Revision) > 0 {
		var revStr string
		if json.Unmarshal(payload.Revision, &revStr) == nil {
			parsed, err := parseRevisionString(revStr)
			if err != nil {
				return protocol.ReminderDelivery{}, err
			}
			expectedRev = parsed
		} else if err := json.Unmarshal(payload.Revision, &expectedRev); err != nil {
			return protocol.ReminderDelivery{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder revision")
		}
	}
	old, err := c.read(ctx, payload.ID)
	if err != nil {
		return protocol.ReminderDelivery{}, err
	}
	now := c.host.Now().UnixMilli()
	if !old.present || old.value.Completed || old.value.NotifiedMs != nil || old.value.DueMs == nil || *old.value.DueMs > now {
		return protocol.ReminderDelivery{Skipped: true}, nil
	}
	expected, err := checkBase(expectedRev, payload.ExpectedValue, old.present, old.revision, old.value)
	if err != nil {
		var f *protocol.Fault
		if errors.As(err, &f) && f.Code == protocol.ErrorCodeRevisionConflict {
			return protocol.ReminderDelivery{Skipped: true}, nil
		}
		return protocol.ReminderDelivery{}, err
	}
	target, err := c.executionNode(ctx, old.value)
	if err != nil {
		return protocol.ReminderDelivery{}, err
	}
	if !sameNode(target, c.host.LocalNodeID()) {
		return protocol.ReminderDelivery{Skipped: true}, nil
	}
	item := old.value
	item.ExecutionNode = &target
	item.NotifiedMs = &now
	k, err := reminderKey(payload.ID)
	if err != nil {
		return protocol.ReminderDelivery{}, err
	}
	encoded, err := marshalRaw(item)
	if err != nil {
		return protocol.ReminderDelivery{}, err
	}
	ops := []protocol.PluginTransactionOperation{
		{
			Kind: "write",
			Data: protocol.PluginTransactionOperationWritePayload{
				Key:              k,
				Value:            encoded,
				ExpectedRevision: expected,
				Scope:            protocol.PluginStorageScopeNode,
			},
		},
		{
			Kind: "notify",
			Data: protocol.NotificationDraft{
				Title:        item.Title,
				Message:      item.Message,
				Kind:         protocol.NotificationKindInfo,
				Session:      nil,
				Presentation: reminderNotificationPresentation,
			},
		},
	}
	if _, err := c.transaction(ctx, ops); err != nil {
		var f *protocol.Fault
		if errors.As(err, &f) && f.Code == protocol.ErrorCodeRevisionConflict {
			return protocol.ReminderDelivery{Skipped: true}, nil
		}
		return protocol.ReminderDelivery{}, err
	}
	return protocol.ReminderDelivery{Delivered: true}, nil
}
