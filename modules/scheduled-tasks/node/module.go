// Package scheduledtasksnode provides the compile-time native Harness Node
// module for personal scheduled Agent tasks. It shares the exact
// "scheduled-tasks" personal storage and dispatch namespace used by earlier
// scheduled-tasks plugin versions.
package scheduledtasksnode

import (
	"context"
	"database/sql"
	_ "embed"
	"encoding/json"
	"errors"
	"maps"
	"strconv"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
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

// Module returns the native Harness Node module providing scheduled-tasks
// commands, persistent dispatch scheduling, and the scheduled_tasks agent tool.
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

type turnScope struct {
	Project   *protocol.ProjectID
	Worktree  *protocol.WorktreeID
	Config    protocol.SessionConfig
	Execution protocol.ConversationExecution
	Team      *protocol.TeamSelection
	Models    map[protocol.RoleID]protocol.RoleModel
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
	if requireConfig(inv.Turn.Config) != nil || inv.Turn.Config.Assistant != nil || inv.Turn.Config.Resource != nil {
		return nil, nil
	}
	scope, err := buildTurnScope(inv)
	if err != nil {
		return nil, err
	}
	client := storageClient{host: host, inv: inv}
	spec.ReadOnly = false
	spec.Concurrent = false
	tool, err := moduleapi.BuildCustomTool(spec, "", func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		return execScheduledTasksTool(ctx, client, scope, call)
	})
	if err != nil {
		return nil, err
	}
	tool.TurnInstruction = prompt.Part{
		Key:  "plugin:" + storageNamespace,
		Text: strings.TrimSpace(catalog.Instructions),
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

func buildTurnScope(inv *store.AgentInvocation) (turnScope, error) {
	var worktree *protocol.WorktreeID
	if inv.Worktree.Project != nil {
		w := inv.Run.Worktree
		worktree = &w
	}
	roster := inv.Turn.Roles
	var team *protocol.TeamSelection
	if roster.Team != nil {
		team = &protocol.TeamSelection{ID: roster.Team.ID, Revision: nil}
	}
	execMode, normalizedTeam, normalizedModels, err := normalizeExecution(roster.Execution, team, roster.Models)
	if err != nil {
		return turnScope{}, err
	}
	return turnScope{
		Project:   inv.Worktree.Project,
		Worktree:  worktree,
		Config:    inv.Turn.Config,
		Execution: execMode,
		Team:      normalizedTeam,
		Models:    normalizedModels,
	}, nil
}

func toolAction(raw json.RawMessage) string {
	var body struct {
		Action string `json:"action"`
	}
	_ = json.Unmarshal(raw, &body)
	return body.Action
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

func formatToolEntry(entry protocol.ScheduledTaskEntry, includeNext bool) map[string]any {
	var rev any
	if entry.Revision != nil {
		rev = strconv.FormatUint(*entry.Revision, 10)
	}
	models := entry.Task.Models
	if models == nil {
		models = map[protocol.RoleID]protocol.RoleModel{}
	}
	out := map[string]any{
		"id":             entry.Task.ID,
		"execution_node": entry.Task.ExecutionNode,
		"name":           entry.Task.Name,
		"prompt":         entry.Task.Prompt,
		"project":        entry.Task.Project,
		"worktree":       entry.Task.Worktree,
		"config":         entry.Task.Config,
		"execution":      entry.Task.Execution,
		"team":           entry.Task.Team,
		"models":         models,
		"timing":         entry.Task.Timing,
		"enabled":        entry.Task.Enabled,
		"queue":          entry.Task.Queue,
		"revision":       rev,
	}
	if includeNext {
		out["next_ms"] = entry.NextMs
	}
	if entry.Unavailable {
		out["unavailable"] = true
	}
	return out
}

func selectExecution(raw map[string]json.RawMessage, prevMode protocol.ConversationExecution, prevTeam *protocol.TeamSelection, prevModels map[protocol.RoleID]protocol.RoleModel) (protocol.ConversationExecution, *protocol.TeamSelection, map[protocol.RoleID]protocol.RoleModel, error) {
	execRaw, hasExec := raw["execution"]
	teamRaw, hasTeam := raw["team"]
	if !hasExec && !hasTeam {
		return prevMode, prevTeam, maps.Clone(prevModels), nil
	}
	mode := prevMode
	if hasExec {
		if isJSONNull(execRaw) {
			return "", nil, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task execution")
		}
		var s string
		if err := json.Unmarshal(execRaw, &s); err != nil {
			return "", nil, nil, fault(protocol.ErrorCodeInvalidRequest, "Invalid task execution")
		}
		mode = protocol.ConversationExecution(s)
	} else if hasTeam {
		mode = protocol.ConversationExecutionTeam
	}
	var team *protocol.TeamSelection
	if hasTeam {
		parsed, err := parseStrictTeam(teamRaw)
		if err != nil {
			return "", nil, nil, err
		}
		team = parsed
	} else if mode == protocol.ConversationExecutionModel {
		team = nil
	} else {
		team = prevTeam
	}
	nextMode, nextTeam, _, err := normalizeExecution(mode, team, nil)
	if err != nil {
		return "", nil, nil, err
	}
	unchanged := prevMode == nextMode && sameTeamID(prevTeam, nextTeam)
	nextModels := map[protocol.RoleID]protocol.RoleModel{}
	if unchanged && len(prevModels) > 0 {
		nextModels = maps.Clone(prevModels)
	}
	return nextMode, nextTeam, nextModels, nil
}

func parseToolTiming(raw json.RawMessage, nowMs int64) (protocol.DispatchTiming, error) {
	schedule, err := parseStrictTiming(raw)
	if err != nil {
		return protocol.DispatchTiming{}, err
	}
	switch schedule.Kind {
	case "once":
		if schedule.Data.(protocol.DispatchTimingOncePayload).AtMs <= nowMs {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Task start time must be in the future")
		}
	case "every":
		if schedule.Data.(protocol.DispatchTimingEveryPayload).AnchorMs <= nowMs {
			return protocol.DispatchTiming{}, fault(protocol.ErrorCodeInvalidRequest, "Task start time must be in the future")
		}
	}
	return schedule, nil
}

func execScheduledTasksTool(ctx context.Context, client storageClient, scope turnScope, call canonical.ToolRequest) engine.ToolResult {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(call.Arguments, &raw); err != nil || raw == nil {
		return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task action or arguments"))
	}
	actionRaw, hasAction := raw["action"]
	if !hasAction || isJSONNull(actionRaw) {
		return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task action or arguments"))
	}
	var action string
	if err := json.Unmarshal(actionRaw, &action); err != nil {
		return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task action or arguments"))
	}
	allowed := map[string]map[string]bool{
		"list":   {"action": true, "after": true},
		"create": {"action": true, "name": true, "prompt": true, "timing": true, "enabled": true, "execution": true, "team": true},
		"update": {"action": true, "id": true, "revision": true, "name": true, "prompt": true, "timing": true, "enabled": true, "execution": true, "team": true},
		"delete": {"action": true, "id": true, "revision": true},
	}
	fields, ok := allowed[action]
	if !ok {
		return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task action or arguments"))
	}
	for k := range raw {
		if !fields[k] {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task action or arguments"))
		}
	}

	switch action {
	case "list":
		var after *string
		if v, has := raw["after"]; has && !isJSONNull(v) {
			var s string
			if err := json.Unmarshal(v, &s); err != nil {
				return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task action or arguments"))
			}
			after = &s
		}
		page, err := client.listing(ctx, after, scope.Project, true)
		if err != nil {
			return toolError(err)
		}
		items := make([]map[string]any, 0, len(page.Items))
		for _, item := range page.Items {
			items = append(items, formatToolEntry(item, true))
		}
		return engine.ToolResult{Value: map[string]any{"items": items, "after": page.After}}

	case "create":
		nameRaw, hasName := raw["name"]
		promptRaw, hasPrompt := raw["prompt"]
		timingRaw, hasTiming := raw["timing"]
		if !hasName || !hasPrompt || !hasTiming {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Task name, prompt and timing are required"))
		}
		if isJSONNull(nameRaw) || isJSONNull(promptRaw) {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid task content or queue"))
		}
		var name, promptText string
		if err := json.Unmarshal(nameRaw, &name); err != nil {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid task content or queue"))
		}
		if err := json.Unmarshal(promptRaw, &promptText); err != nil {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid task content or queue"))
		}
		timing, err := parseToolTiming(timingRaw, client.host.Now().UnixMilli())
		if err != nil {
			return toolError(err)
		}
		enabled := true
		if v, has := raw["enabled"]; has {
			if isJSONNull(v) || json.Unmarshal(v, &enabled) != nil {
				return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid task content or queue"))
			}
		}
		execMode, team, models, err := selectExecution(raw, scope.Execution, scope.Team, scope.Models)
		if err != nil {
			return toolError(err)
		}
		item, ops, err := client.saveOperations(ctx, protocol.ScheduledTask{
			ID:        newID(),
			Name:      name,
			Prompt:    promptText,
			Project:   scope.Project,
			Worktree:  scope.Worktree,
			Config:    scope.Config,
			Execution: execMode,
			Team:      team,
			Models:    models,
			Timing:    timing,
			Enabled:   enabled,
			Queue:     "default",
		}, 0, nil, scope.Project, true)
		if err != nil {
			return toolError(err)
		}
		outputs, err := client.transaction(ctx, ops)
		if err != nil {
			return toolError(err)
		}
		written, err := extractWrittenEntry(outputs, item.ID)
		if err != nil || !written.Present {
			return toolError(fault(protocol.ErrorCodeOutcomeUnknown, "Task outcome is unavailable"))
		}
		rev := written.Revision
		return engine.ToolResult{Value: formatToolEntry(protocol.ScheduledTaskEntry{Task: item, Revision: &rev}, false)}

	case "update":
		id, rev, err := parseIDAndRevision(raw)
		if err != nil {
			return toolError(err)
		}
		hasChange := false
		for _, k := range []string{"name", "prompt", "timing", "enabled", "execution", "team"} {
			if _, exists := raw[k]; exists {
				hasChange = true
				break
			}
		}
		if !hasChange {
			return toolError(fault(protocol.ErrorCodeInvalidRequest, "Task update requires a change"))
		}
		var newTiming *protocol.DispatchTiming
		if timingRaw, has := raw["timing"]; has {
			parsed, err := parseToolTiming(timingRaw, client.host.Now().UnixMilli())
			if err != nil {
				return toolError(err)
			}
			newTiming = &parsed
		}
		old, err := client.read(ctx, id)
		if err != nil {
			return toolError(err)
		}
		if !old.present {
			return toolError(fault(protocol.ErrorCodeNotFound, "Task is unavailable"))
		}
		if !sameProject(old.value.Project, scope.Project) {
			return toolError(fault(protocol.ErrorCodePermissionDenied, "Task belongs to another project"))
		}
		target, err := client.executionNode(ctx, old.value)
		if err != nil {
			return toolError(err)
		}
		local := client.host.LocalNodeID()
		if !sameNode(target, local) {
			out, err := client.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "read_scheduled_task",
				Data: &protocol.CommandReadScheduledTaskPayload{ID: id},
			})
			if err != nil {
				return toolError(err)
			}
			current, ok := unwrapScheduledTaskEntry(out.Data)
			if !ok || current.Revision == nil {
				return toolError(fault(protocol.ErrorCodeNotFound, "Task is unavailable"))
			}
			if !sameProject(current.Task.Project, scope.Project) {
				return toolError(fault(protocol.ErrorCodePermissionDenied, "Task belongs to another project"))
			}
			if *current.Revision != rev {
				return toolError(fault(protocol.ErrorCodeRevisionConflict, "Task changed"))
			}
			updated, err := applyTaskPatch(current.Task, raw, newTiming)
			if err != nil {
				return toolError(err)
			}
			updated.ExecutionNode = &target
			expectedVal := current.Task
			savedOut, err := client.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "save_scheduled_task",
				Data: &protocol.CommandSaveScheduledTaskPayload{
					Task:          updated,
					Revision:      *current.Revision,
					ExpectedValue: &expectedVal,
				},
			})
			if err != nil {
				return toolError(err)
			}
			saved, ok := unwrapScheduledTaskEntry(savedOut.Data)
			if !ok {
				return toolError(fault(protocol.ErrorCodeOutcomeUnknown, "Task outcome is unavailable"))
			}
			return engine.ToolResult{Value: formatToolEntry(saved, false)}
		}
		updated, err := applyTaskPatch(old.value, raw, newTiming)
		if err != nil {
			return toolError(err)
		}
		item, ops, err := client.saveOperations(ctx, updated, rev, nil, scope.Project, true)
		if err != nil {
			return toolError(err)
		}
		outputs, err := client.transaction(ctx, ops)
		if err != nil {
			return toolError(err)
		}
		written, err := extractWrittenEntry(outputs, item.ID)
		if err != nil || !written.Present {
			return toolError(fault(protocol.ErrorCodeOutcomeUnknown, "Task outcome is unavailable"))
		}
		writtenRev := written.Revision
		return engine.ToolResult{Value: formatToolEntry(protocol.ScheduledTaskEntry{Task: item, Revision: &writtenRev}, false)}

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
			return toolError(fault(protocol.ErrorCodeNotFound, "Task is unavailable"))
		}
		if !sameProject(old.value.Project, scope.Project) {
			return toolError(fault(protocol.ErrorCodePermissionDenied, "Task belongs to another project"))
		}
		target, err := client.executionNode(ctx, old.value)
		if err != nil {
			return toolError(err)
		}
		local := client.host.LocalNodeID()
		if !sameNode(target, local) {
			out, err := client.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "read_scheduled_task",
				Data: &protocol.CommandReadScheduledTaskPayload{ID: id},
			})
			if err != nil {
				return toolError(err)
			}
			current, ok := unwrapScheduledTaskEntry(out.Data)
			if !ok || current.Revision == nil {
				return toolError(fault(protocol.ErrorCodeNotFound, "Task is unavailable"))
			}
			if !sameProject(current.Task.Project, scope.Project) {
				return toolError(fault(protocol.ErrorCodePermissionDenied, "Task belongs to another project"))
			}
			if *current.Revision != rev {
				return toolError(fault(protocol.ErrorCodeRevisionConflict, "Task changed"))
			}
			expectedVal := current.Task
			if _, err := client.host.ForwardModuleCommand(ctx, target, protocol.Command{
				Kind: "remove_scheduled_task",
				Data: &protocol.CommandRemoveScheduledTaskPayload{
					ID:            id,
					Revision:      *current.Revision,
					ExpectedValue: &expectedVal,
				},
			}); err != nil {
				return toolError(err)
			}
			return engine.ToolResult{Value: map[string]any{"id": id, "removed": true}}
		}
		ops, err := client.removeOperations(ctx, id, rev, nil, scope.Project, true)
		if err != nil {
			return toolError(err)
		}
		outputs, err := client.transaction(ctx, ops)
		if err != nil {
			return toolError(err)
		}
		removed, err := extractWrittenEntry(outputs, id)
		if err != nil || removed.Present {
			return toolError(fault(protocol.ErrorCodeOutcomeUnknown, "Task outcome is unavailable"))
		}
		return engine.ToolResult{Value: map[string]any{
			"id":       id,
			"revision": strconv.FormatUint(removed.Revision, 10),
			"removed":  true,
		}}
	}
	return toolError(fault(protocol.ErrorCodeInvalidRequest, "Invalid scheduled task action or arguments"))
}

func parseIDAndRevision(raw map[string]json.RawMessage) (string, uint64, error) {
	idRaw, hasID := raw["id"]
	revRaw, hasRev := raw["revision"]
	if !hasID || !hasRev || isJSONNull(idRaw) || isJSONNull(revRaw) {
		return "", 0, fault(protocol.ErrorCodeInvalidRequest, "Task ID and revision are required")
	}
	var id, revStr string
	if err := json.Unmarshal(idRaw, &id); err != nil || id == "" {
		return "", 0, fault(protocol.ErrorCodeInvalidRequest, "Task ID and revision are required")
	}
	if err := json.Unmarshal(revRaw, &revStr); err != nil {
		return "", 0, fault(protocol.ErrorCodeInvalidRequest, "Task ID and revision are required")
	}
	rev, err := parseRevisionString(revStr)
	if err != nil {
		return "", 0, err
	}
	return id, rev, nil
}

func applyTaskPatch(base protocol.ScheduledTask, raw map[string]json.RawMessage, newTiming *protocol.DispatchTiming) (protocol.ScheduledTask, error) {
	execMode, team, models, err := selectExecution(raw, base.Execution, base.Team, base.Models)
	if err != nil {
		return protocol.ScheduledTask{}, err
	}
	next := base
	next.Execution = execMode
	next.Team = team
	next.Models = models
	if v, has := raw["name"]; has {
		if isJSONNull(v) || json.Unmarshal(v, &next.Name) != nil {
			return protocol.ScheduledTask{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task content or queue")
		}
	}
	if v, has := raw["prompt"]; has {
		if isJSONNull(v) || json.Unmarshal(v, &next.Prompt) != nil {
			return protocol.ScheduledTask{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task content or queue")
		}
	}
	if v, has := raw["enabled"]; has {
		if isJSONNull(v) || json.Unmarshal(v, &next.Enabled) != nil {
			return protocol.ScheduledTask{}, fault(protocol.ErrorCodeInvalidRequest, "Invalid task content or queue")
		}
	}
	if newTiming != nil {
		next.Timing = *newTiming
	}
	return next, nil
}
