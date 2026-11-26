// Package remindersnode provides the compile-time native Harness Node module
// for personal to-dos and scheduled reminder notifications. It shares the exact
// "reminders" personal storage and dispatch namespace used by earlier reminders
// plugin versions.
package remindersnode

import (
	"bytes"
	"context"
	"database/sql"
	_ "embed"
	"encoding/json"
	"errors"
	"strconv"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed tools.json
var catalogJSON []byte

var loadCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	return moduleapi.LoadCatalog(catalogJSON)
})

type module struct{}

// Module returns the native Harness Node module providing reminders commands,
// scheduled reminder delivery, and the reminders agent tool.
func Module() moduleapi.Module { return module{} }

var (
	_ moduleapi.CommandHandler   = module{}
	_ moduleapi.DispatchProvider = module{}
)

func (module) ID() string                      { return storageNamespace }
func (module) ReplacesPlugins() []string       { return []string{storageNamespace} }
func (module) NativeDispatchNamespace() string { return storageNamespace }

func (module) Catalog() []moduleapi.ToolMeta {
	catalog, err := loadCatalog()
	if err != nil {
		return nil
	}
	metas := make([]moduleapi.ToolMeta, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		metas = append(metas, spec.Meta())
	}
	return metas
}

func (module) ToolsForTurn(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if installed, _, err := host.PluginInstalled(ctx, storageNamespace); err != nil {
		return nil, err
	} else if installed {
		if _, captured := host.PluginCaptured(inv, storageNamespace); !captured {
			return nil, nil
		}
	}
	catalog, err := loadCatalog()
	if err != nil || len(catalog.Tools) == 0 {
		return nil, err
	}
	spec := catalog.Tools[0]
	if !spec.Available(inv) {
		return nil, nil
	}
	client := storageClient{host: host, inv: inv}
	scopeProject := inv.Worktree.Project
	spec.ReadOnly = false
	spec.Concurrent = false
	tool, err := moduleapi.BuildCustomTool(spec, catalog.Instructions, func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		return execRemindersTool(ctx, client, scopeProject, call)
	})
	if err != nil {
		return nil, err
	}
	tool.Approval = false
	tool.Confirm = func(call canonical.ToolRequest) bool {
		return toolAction(call.Arguments) != "list"
	}
	tool.Binding = &store.ApprovalBinding{Tool: spec.Name}
	tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, _ canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
		return host.ConfigureStorageApproval(ctx, db, inv.Run.Turn, value)
	}
	return []engine.Tool{tool}, nil
}

func toolAction(raw json.RawMessage) string {
	var body struct {
		Action string `json:"action"`
	}
	_ = json.Unmarshal(raw, &body)
	return body.Action
}

func isJSONNull(raw json.RawMessage) bool {
	return bytes.Equal(bytes.TrimSpace(raw), []byte("null"))
}

func toolError(err error) engine.ToolResult {
	var f *protocol.Fault
	code := string(protocol.ErrorCodeInvalidRequest)
	msg := err.Error()
	if errors.As(err, &f) {
		code = string(f.Code)
		msg = f.Message
	}
	return engine.ToolResult{
		ReturnedError: true,
		Value: map[string]any{
			"isError": true,
			"error": map[string]any{
				"code":    code,
				"message": msg,
			},
		},
	}
}

func formatToolEntry(entry protocol.ReminderEntry) map[string]any {
	var rev any
	if entry.Revision != nil {
		rev = strconv.FormatUint(*entry.Revision, 10)
	}
	out := map[string]any{
		"id":             entry.Reminder.ID,
		"execution_node": entry.Reminder.ExecutionNode,
		"project":        entry.Reminder.Project,
		"title":          entry.Reminder.Title,
		"message":        entry.Reminder.Message,
		"completed":      entry.Reminder.Completed,
		"due_ms":         entry.Reminder.DueMs,
		"notified_ms":    entry.Reminder.NotifiedMs,
		"revision":       rev,
	}
	if entry.Unavailable {
		out["unavailable"] = true
	}
	return out
}

func execRemindersTool(ctx context.Context, client storageClient, scopeProject *protocol.ProjectID, call canonical.ToolRequest) engine.ToolResult {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(call.Arguments, &raw); err != nil || raw == nil {
		return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder action or arguments"))
	}
	var action string
	if err := json.Unmarshal(raw["action"], &action); err != nil {
		return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder action or arguments"))
	}
	allowed := map[string]map[string]bool{
		"list":   {"action": true, "after": true, "completed": true},
		"create": {"action": true, "title": true, "message": true, "completed": true, "due_ms": true},
		"update": {"action": true, "id": true, "revision": true, "title": true, "message": true, "completed": true, "due_ms": true},
		"delete": {"action": true, "id": true, "revision": true},
	}
	fields, ok := allowed[action]
	if !ok {
		return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder action or arguments"))
	}
	for k := range raw {
		if !fields[k] {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder action or arguments"))
		}
	}

	switch action {
	case "list":
		var after *string
		var completed *bool
		if v, has := raw["after"]; has && !isJSONNull(v) {
			var s string
			if err := json.Unmarshal(v, &s); err != nil {
				return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder ID"))
			}
			after = &s
		}
		if v, has := raw["completed"]; has {
			var b bool
			if isJSONNull(v) || json.Unmarshal(v, &b) != nil {
				return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder completion filter"))
			}
			completed = &b
		}
		page, err := client.page(ctx, after, completed, scopeProject, true)
		if err != nil {
			return toolError(err)
		}
		items := make([]map[string]any, 0, len(page.Items))
		for _, item := range page.Items {
			items = append(items, formatToolEntry(item))
		}
		return engine.ToolResult{Value: map[string]any{"items": items, "after": page.After}}

	case "create":
		titleRaw, has := raw["title"]
		if !has || isJSONNull(titleRaw) {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Reminder title is required"))
		}
		var title, message string
		var completed bool
		var dueMs *int64
		if err := json.Unmarshal(titleRaw, &title); err != nil {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Reminder title is required"))
		}
		if v, has := raw["message"]; has {
			if isJSONNull(v) || json.Unmarshal(v, &message) != nil {
				return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time"))
			}
		}
		if v, has := raw["completed"]; has {
			if isJSONNull(v) || json.Unmarshal(v, &completed) != nil {
				return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time"))
			}
		}
		if v, has := raw["due_ms"]; has && !isJSONNull(v) {
			var ms int64
			if err := json.Unmarshal(v, &ms); err != nil {
				return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time"))
			}
			dueMs = &ms
		}
		item, ops, err := client.saveOperations(ctx, protocol.Reminder{
			ID:        newID(),
			Project:   scopeProject,
			Title:     title,
			Message:   message,
			Completed: completed,
			DueMs:     dueMs,
		}, 0, nil, scopeProject, true)
		if err != nil {
			return toolError(err)
		}
		outputs, err := client.transaction(ctx, ops)
		if err != nil {
			return toolError(err)
		}
		written, err := extractWrittenEntry(outputs, item.ID)
		if err != nil || !written.Present {
			return toolError(fault(protocol.ErrorCodeOutcomeUnknown, "Reminder outcome is unavailable"))
		}
		rev := written.Revision
		return engine.ToolResult{Value: formatToolEntry(protocol.ReminderEntry{Reminder: item, Revision: &rev})}

	case "update":
		id, rev, err := parseIDAndRevision(raw)
		if err != nil {
			return toolError(err)
		}
		hasChange := false
		for _, k := range []string{"title", "message", "completed", "due_ms"} {
			if _, ok := raw[k]; ok {
				hasChange = true
				break
			}
		}
		if !hasChange {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Reminder update requires a change"))
		}
		old, err := client.read(ctx, id)
		if err != nil {
			return toolError(err)
		}
		if !old.present {
			return toolError(fault(protocol.ErrorCodeNotFound, "Reminder is unavailable"))
		}
		if !sameProject(old.value.Project, scopeProject) {
			return toolError(fault(protocol.ErrorCodePermissionDenied, "Reminder belongs to another project"))
		}
		target, err := client.executionNode(ctx, old.value)
		if err != nil {
			return toolError(err)
		}
		local := client.host.LocalNodeID()
		if !sameNode(target, local) {
			out, err := client.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "read_reminder",
				Data: &protocol.CommandReadReminderPayload{ID: id},
			})
			if err != nil {
				return toolError(err)
			}
			current, ok := unwrapReminderEntry(out.Data)
			if !ok || current.Revision == nil {
				return toolError(fault(protocol.ErrorCodeNotFound, "Reminder is unavailable"))
			}
			if !sameProject(current.Reminder.Project, scopeProject) {
				return toolError(fault(protocol.ErrorCodePermissionDenied, "Reminder belongs to another project"))
			}
			if *current.Revision != rev {
				return toolError(fault(protocol.ErrorCodeRevisionConflict, "Reminder changed"))
			}
			updated, err := applyReminderPatch(current.Reminder, raw)
			if err != nil {
				return toolError(err)
			}
			updated.ExecutionNode = &target
			expectedVal := current.Reminder
			savedOut, err := client.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "save_reminder",
				Data: &protocol.CommandSaveReminderPayload{
					Reminder:      updated,
					Revision:      *current.Revision,
					ExpectedValue: &expectedVal,
				},
			})
			if err != nil {
				return toolError(err)
			}
			saved, ok := unwrapReminderEntry(savedOut.Data)
			if !ok {
				return toolError(fault(protocol.ErrorCodeOutcomeUnknown, "Reminder outcome is unavailable"))
			}
			return engine.ToolResult{Value: formatToolEntry(saved)}
		}
		updated, err := applyReminderPatch(old.value, raw)
		if err != nil {
			return toolError(err)
		}
		item, ops, err := client.saveOperations(ctx, updated, rev, nil, scopeProject, true)
		if err != nil {
			return toolError(err)
		}
		outputs, err := client.transaction(ctx, ops)
		if err != nil {
			return toolError(err)
		}
		written, err := extractWrittenEntry(outputs, item.ID)
		if err != nil || !written.Present {
			return toolError(fault(protocol.ErrorCodeOutcomeUnknown, "Reminder outcome is unavailable"))
		}
		writtenRev := written.Revision
		return engine.ToolResult{Value: formatToolEntry(protocol.ReminderEntry{Reminder: item, Revision: &writtenRev})}

	case "delete":
		id, rev, err := parseIDAndRevision(raw)
		if err != nil {
			return toolError(err)
		}
		old, err := client.read(ctx, id)
		if err != nil {
			return toolError(err)
		}
		if !old.present {
			return toolError(fault(protocol.ErrorCodeNotFound, "Reminder is unavailable"))
		}
		if !sameProject(old.value.Project, scopeProject) {
			return toolError(fault(protocol.ErrorCodePermissionDenied, "Reminder belongs to another project"))
		}
		target, err := client.executionNode(ctx, old.value)
		if err != nil {
			return toolError(err)
		}
		local := client.host.LocalNodeID()
		if !sameNode(target, local) {
			out, err := client.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "read_reminder",
				Data: &protocol.CommandReadReminderPayload{ID: id},
			})
			if err != nil {
				return toolError(err)
			}
			current, ok := unwrapReminderEntry(out.Data)
			if !ok || current.Revision == nil {
				return toolError(fault(protocol.ErrorCodeNotFound, "Reminder is unavailable"))
			}
			if !sameProject(current.Reminder.Project, scopeProject) {
				return toolError(fault(protocol.ErrorCodePermissionDenied, "Reminder belongs to another project"))
			}
			if *current.Revision != rev {
				return toolError(fault(protocol.ErrorCodeRevisionConflict, "Reminder changed"))
			}
			expectedVal := current.Reminder
			if _, err := client.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "remove_reminder",
				Data: &protocol.CommandRemoveReminderPayload{
					ID:            id,
					Revision:      *current.Revision,
					ExpectedValue: &expectedVal,
				},
			}); err != nil {
				return toolError(err)
			}
			return engine.ToolResult{Value: map[string]any{"id": id, "removed": true}}
		}
		ops, err := client.removeOperations(ctx, id, rev, nil, scopeProject, true)
		if err != nil {
			return toolError(err)
		}
		outputs, err := client.transaction(ctx, ops)
		if err != nil {
			return toolError(err)
		}
		removed, err := extractWrittenEntry(outputs, id)
		if err != nil || removed.Present {
			return toolError(fault(protocol.ErrorCodeOutcomeUnknown, "Reminder outcome is unavailable"))
		}
		return engine.ToolResult{Value: map[string]any{
			"id":       id,
			"revision": strconv.FormatUint(removed.Revision, 10),
			"removed":  true,
		}}
	}
	return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder action or arguments"))
}

func parseIDAndRevision(raw map[string]json.RawMessage) (string, uint64, error) {
	var id, revStr string
	if err := json.Unmarshal(raw["id"], &id); err != nil || id == "" {
		return "", 0, fault(protocol.ErrorCodeInvalidRequest, "Reminder ID and revision are required")
	}
	if err := json.Unmarshal(raw["revision"], &revStr); err != nil {
		return "", 0, fault(protocol.ErrorCodeInvalidRequest, "Reminder ID and revision are required")
	}
	rev, err := parseRevisionString(revStr)
	if err != nil {
		return "", 0, err
	}
	return id, rev, nil
}

func applyReminderPatch(base protocol.Reminder, raw map[string]json.RawMessage) (protocol.Reminder, error) {
	next := base
	if v, has := raw["title"]; has {
		if isJSONNull(v) || json.Unmarshal(v, &next.Title) != nil {
			return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time")
		}
	}
	if v, has := raw["message"]; has {
		if isJSONNull(v) || json.Unmarshal(v, &next.Message) != nil {
			return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time")
		}
	}
	if v, has := raw["completed"]; has {
		if isJSONNull(v) || json.Unmarshal(v, &next.Completed) != nil {
			return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time")
		}
	}
	if v, has := raw["due_ms"]; has {
		if isJSONNull(v) {
			next.DueMs = nil
		} else {
			var ms int64
			if err := json.Unmarshal(v, &ms); err != nil {
				return protocol.Reminder{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid reminder content or time")
			}
			next.DueMs = &ms
		}
	}
	return next, nil
}
