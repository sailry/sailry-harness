package node

import (
	"context"
	_ "embed"
	"encoding/json"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed goal_tool_locales.json
var goalToolLocalization []byte

var goalToolLabels = sync.OnceValue(func() map[string]protocol.ToolResultLocalText {
	var labels map[string]protocol.ToolResultLocalText
	if err := json.Unmarshal(goalToolLocalization, &labels); err != nil {
		panic(err)
	}
	return labels
})

// goalState keeps the conversation value understood by the goal controls and
// continuation hook. Existing values also keep their revision and turn anchor.
type goalState struct {
	ID           string     `json:"id"`
	Objective    string     `json:"objective"`
	Version      uint64     `json:"version"`
	Status       string     `json:"status"`
	Reason       *string    `json:"reason"`
	Detail       *string    `json:"detail"`
	Sequence     uint64     `json:"seq"`
	HeldThrough  uint64     `json:"held_through"`
	Waiting      bool       `json:"waiting"`
	Claim        *goalClaim `json:"claim"`
	Evidence     *string    `json:"evidence"`
	Edited       bool       `json:"edited"`
	Wrap         *string    `json:"wrap"`
	Idle         uint64     `json:"idle"`
	Notice       *string    `json:"notice"`
	Turns        uint64     `json:"turns"`
	Chain        uint64     `json:"chain"`
	Tokens       uint64     `json:"tokens"`
	WindowTokens uint64     `json:"window_tokens"`
	ElapsedMs    int64      `json:"elapsed_ms"`
	WindowMs     int64      `json:"window_ms"`
	SinceMs      *int64     `json:"since_ms"`
	CreatedMs    int64      `json:"created_ms"`
	FinishedMs   *int64     `json:"finished_ms"`
	saved        json.RawMessage
}

type goalClaim struct {
	Note string `json:"note"`
}

func (goal *goalState) MarshalJSON() ([]byte, error) {
	type stored goalState
	current, err := json.Marshal((*stored)(goal))
	if err != nil || len(goal.saved) == 0 {
		return current, err
	}
	// Updates retain fields outside the model tools' current state format.
	var previous map[string]json.RawMessage
	if err := json.Unmarshal(goal.saved, &previous); err != nil {
		return nil, err
	}
	if err := json.Unmarshal(current, &previous); err != nil {
		return nil, err
	}
	return json.Marshal(previous)
}

type goalToolLimits struct {
	Turns, Tokens, Minutes uint64
}

type goalToolScope struct {
	node       *Node
	invocation *store.AgentInvocation
	plugin     protocol.PluginContext
	name       string
}

func (node *Node) coreGoalExecutor(invocation *store.AgentInvocation, info protocol.PluginInfo, name string) func(context.Context, canonical.ToolRequest) engine.ToolResult {
	official := info.Origin != nil && (info.Origin.Kind == "bundled" || info.Origin.Kind == "official") ||
		info.UpdateSource != nil && info.UpdateSource.Kind == "official"
	if info.Summary.Name != "goals" || !official {
		return nil
	}
	if name != "get_goal" && name != "create_goal" && name != "update_goal" {
		return nil
	}
	scope := goalToolScope{node: node, invocation: invocation, name: name, plugin: protocol.PluginContext{
		Package: plugins.PackageReference(info), Turn: &invocation.Run.Turn,
		Session: &invocation.Session.ID, Worktree: &invocation.Run.Worktree,
		Surface: protocol.PluginDesktopSurface{Kind: "workspace"},
	}}
	return func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		ctx = store.WithCapturedUser(store.WithUser(ctx, invocation.User), invocation.StorageUser)
		var result map[string]any
		var err error
		switch name {
		case "get_goal":
			result, err = scope.read(ctx)
		case "create_goal":
			result, err = scope.create(ctx, call)
		case "update_goal":
			result, err = scope.update(ctx, call)
		}
		if err != nil {
			return workspaceToolFault(err)
		}
		return engine.ToolResult{Value: result}
	}
}

func (scope goalToolScope) request(kind string, payload any) protocol.Request {
	request := protocol.NewRequest(scope.node.ID(), protocol.Command{Kind: kind, Data: payload})
	request.Plugin = &scope.plugin
	return request
}

func goalRead[T any](ctx context.Context, scope goalToolScope, kind string, payload any, expected string) (T, error) {
	var zero T
	output, err := scope.node.dispatchCallbackResult(ctx, scope.request(kind, payload))
	if err != nil {
		return zero, err
	}
	value, ok := dispatchPayload[T](output.Data)
	if !ok || output.Kind != expected {
		return zero, goalToolFault(protocol.ErrorCodeInternal, "goal read returned an unexpected result")
	}
	return value, nil
}

func (scope goalToolScope) value(ctx context.Context, key string) (protocol.PluginStorageConversationEntry, error) {
	return goalRead[protocol.PluginStorageConversationEntry](ctx, scope, "read_plugin_conversation_value",
		&protocol.CommandReadPluginConversationValuePayload{Key: key}, "plugin_conversation_value")
}

func (scope goalToolScope) limits(ctx context.Context) goalToolLimits {
	settings, _ := goalRead[protocol.PluginSettingsState](ctx, scope, "read_plugin_settings",
		&protocol.CommandReadPluginSettingsPayload{Package: scope.plugin.Package}, "plugin_settings")
	value := func(key string, fallback, low, high uint64) uint64 {
		var number uint64
		if raw := settings.Values[key]; len(raw) == 0 || json.Unmarshal(raw, &number) != nil || number < low || number > high {
			return fallback
		}
		return number
	}
	return goalToolLimits{Turns: value("turn_limit", 25, 1, 100), Tokens: value("token_budget", 0, 0, 1_000_000_000) * 1000,
		Minutes: value("time_limit", 0, 0, 100_000)}
}

func readGoalValue(entry protocol.PluginStorageConversationEntry) (*goalState, error) {
	if !entry.Present || string(entry.Value) == "null" {
		return nil, nil
	}
	var goal goalState
	if err := json.Unmarshal(entry.Value, &goal); err != nil {
		return nil, goalToolFault(protocol.ErrorCodeInternal, "stored goal is invalid")
	}
	if goal.Objective != "" {
		goal.saved = entry.Value
		return &goal, nil
	}
	// Values from the first goal package have a description and state.
	var old struct{ Description, State string }
	if err := json.Unmarshal(entry.Value, &old); err != nil || old.Description == "" {
		return nil, goalToolFault(protocol.ErrorCodeInternal, "stored goal is invalid")
	}
	goal = goalState{ID: goal.ID, Objective: old.Description, Version: 1, Status: old.State}
	if goal.Status != "completed" && goal.Status != "blocked" {
		goal.Status, goal.Reason = "paused", new("stopped")
	}
	return &goal, nil
}

func (goal *goalState) stickyPause() bool {
	return goal.Status == "paused" && goal.Reason != nil && (*goal.Reason == "user" || *goal.Reason == "restored")
}

func (goal *goalState) pause(reason string) {
	goal.Status, goal.Reason, goal.Detail = "paused", &reason, nil
	goal.resetActivity()
}

func (goal *goalState) resetActivity() {
	goal.Waiting, goal.Claim, goal.Wrap, goal.Idle = false, nil, nil, 0
}

func (goal *goalState) resume() {
	if goal.Reason != nil {
		switch *goal.Reason {
		case "tokens":
			goal.WindowTokens = 0
		case "time":
			goal.WindowMs = 0
		}
	}
	goal.Status, goal.Reason, goal.Detail = "active", nil, nil
	goal.Evidence, goal.FinishedMs, goal.Notice = nil, nil, nil
	goal.resetActivity()
}

func (goal *goalState) openTime(now int64) int64 {
	if goal.SinceMs == nil {
		return 0
	}
	return max(0, now-*goal.SinceMs)
}

func (goal *goalState) accountTime(now int64) {
	spent := goal.openTime(now)
	goal.ElapsedMs += spent
	goal.WindowMs += spent
	goal.SinceMs = nil
	if goal.Status == "active" {
		goal.SinceMs = &now
	}
}

func (scope goalToolScope) read(ctx context.Context) (map[string]any, error) {
	entry, err := scope.value(ctx, "goal")
	if err != nil {
		return nil, err
	}
	goal, err := readGoalValue(entry)
	if err != nil || goal == nil {
		return map[string]any{"goal": nil}, err
	}
	limits := scope.limits(ctx)
	if entry.Restored && goal.Status != "completed" && !goal.stickyPause() {
		goal.pause("restored")
		goal.SinceMs = nil
	} else if goal.Status == "active" {
		page, err := goalRead[protocol.ConversationRunsPage](ctx, scope, "read_conversation_runs", &protocol.CommandReadConversationRunsPayload{Session: scope.invocation.Session.ID, Limit: 8}, "conversation_runs")
		if err != nil {
			return nil, err
		}
		for _, run := range page.Runs {
			if run.Origin != nil {
				continue
			}
			reason := ""
			switch run.Status {
			case protocol.ConversationStatusInterrupted:
				reason = "restart"
			case protocol.ConversationStatusFailed:
				reason = "error"
			case protocol.ConversationStatusCancelled:
				reason = "stopped"
				if run.ErrorCode != nil && *run.ErrorCode == protocol.ErrorCodeOutcomeUnknown {
					reason = "restart"
				}
			default:
				if run.Recovered {
					reason = "restart"
				}
			}
			if reason == "" && goal.Waiting && run.Sequence <= goal.Sequence && page.Background != nil && page.Background.Running == 0 && page.Background.Pending == 0 {
				reason = "stopped"
				if goal.Chain >= limits.Turns {
					reason = "turns"
				}
			}
			if reason != "" {
				goal.pause(reason)
				goal.SinceMs = nil
			}
			break
		}
	}
	return goalResult(goal, entry.Revision, limits, time.Now().UnixMilli()), nil
}

func (scope goalToolScope) create(ctx context.Context, call canonical.ToolRequest) (map[string]any, error) {
	if scope.automatic() {
		return nil, goalToolFault(protocol.ErrorCodePermissionDenied, "Only the user can start a goal. Ask them first")
	}
	var input struct{ Objective string }
	if err := agentArguments(call.Arguments, &input); err != nil {
		return nil, err
	}
	entry, err := scope.value(ctx, "goal")
	if err != nil {
		return nil, err
	}
	previous, err := readGoalValue(entry)
	if err != nil {
		return nil, err
	}
	if previous != nil && previous.Status != "completed" {
		return nil, goalToolFault(protocol.ErrorCodeConflict, "A goal is already set. Only the user can replace it, with /goal <text>")
	}
	objective := strings.TrimSpace(input.Objective)
	if objective == "" {
		return nil, goalToolFault(protocol.ErrorCodeInvalidRequest, "Describe the goal")
	}
	if len(objective) > 16*1024 {
		return nil, goalToolFault(protocol.ErrorCodeInvalidRequest, "The goal is longer than 16 KiB")
	}
	now := time.Now().UnixMilli()
	goal := &goalState{ID: protocol.NewRequestID().String(), Objective: objective, Version: 1, Status: "active", SinceMs: &now, CreatedMs: now}
	var ops []protocol.PluginTransactionOperation
	if previous != nil {
		archive, err := scope.archive(ctx, previous, now)
		if err != nil {
			return nil, err
		}
		ops = append(ops, archive)
	}
	if err := scope.write(ctx, call, entry, goal, ops); err != nil {
		return nil, err
	}
	return goalResult(goal, entry.Revision+1, scope.limits(ctx), now), nil
}

func (scope goalToolScope) automatic() bool {
	return scope.invocation.Run.Notice || scope.invocation.Run.Continuation != nil
}

func (scope goalToolScope) update(ctx context.Context, call canonical.ToolRequest) (map[string]any, error) {
	var input struct{ Status, Note string }
	if err := agentArguments(call.Arguments, &input); err != nil {
		return nil, err
	}
	entry, err := scope.value(ctx, "goal")
	if err != nil {
		return nil, err
	}
	goal, err := readGoalValue(entry)
	if err != nil {
		return nil, err
	}
	if goal == nil {
		return nil, goalToolFault(protocol.ErrorCodeNotFound, "No goal is set")
	}
	if entry.Restored && goal.Status != "completed" && !goal.stickyPause() {
		goal.pause("restored")
		goal.SinceMs = nil
	}
	now := time.Now().UnixMilli()
	if err := scope.change(goal, input.Status, input.Note, now); err != nil {
		return nil, err
	}
	goal.accountTime(now)
	if err := scope.write(ctx, call, entry, goal, nil); err != nil {
		return nil, err
	}
	result := goalResult(goal, entry.Revision+1, scope.limits(ctx), now)
	if input.Status == "complete" && goal.Status == "active" {
		result["next"] = "The claim is recorded. End this turn with a short summary; a verification turn follows."
	}
	return result, nil
}

func (scope goalToolScope) change(goal *goalState, status, note string, now int64) error {
	note = clipGoalNote(strings.TrimSpace(note), 4096)
	if status == "paused" && scope.automatic() {
		return goalToolFault(protocol.ErrorCodePermissionDenied, "Only the user can pause the goal")
	}
	if (status == "complete" || status == "blocked" || status == "paused") && goal.Status != "active" {
		return goalToolFault(protocol.ErrorCodeConflict, "The goal is not active")
	}
	switch status {
	case "complete":
		if note == "" {
			return goalToolFault(protocol.ErrorCodeInvalidRequest, "State the evidence in note")
		}
		continuation := scope.invocation.Run.Continuation
		if continuation != nil && continuation.Package.Name == scope.plugin.Package.Name && continuation.Kind != nil && *continuation.Kind == "verify" && goal.Claim != nil {
			goal.Status, goal.Reason, goal.Detail = "completed", nil, nil
			goal.resetActivity()
			goal.Evidence, goal.FinishedMs, goal.Notice = &note, &now, new("completed")
		} else {
			goal.Claim = &goalClaim{Note: note}
		}
	case "blocked":
		if note == "" {
			return goalToolFault(protocol.ErrorCodeInvalidRequest, "Say in note what is needed")
		}
		goal.Status, goal.Reason, goal.Detail, goal.Notice = "blocked", nil, &note, new("blocked")
		goal.resetActivity()
	case "paused":
		goal.pause("user")
	case "active":
		if scope.automatic() {
			return goalToolFault(protocol.ErrorCodePermissionDenied, "Only the user can resume the goal")
		}
		switch goal.Status {
		case "completed":
			return goalToolFault(protocol.ErrorCodePermissionDenied, "Only the user can reopen a completed goal")
		case "active":
			return goalToolFault(protocol.ErrorCodeConflict, "The goal is already active")
		}
		goal.resume()
	default:
		return goalToolFault(protocol.ErrorCodeInvalidRequest, "Invalid goal status")
	}
	return nil
}

func clipGoalNote(text string, bytes int) string {
	if len(text) <= bytes {
		return text
	}
	for !utf8.RuneStart(text[bytes]) {
		bytes--
	}
	return strings.Clone(text[:bytes])
}

func (scope goalToolScope) archive(ctx context.Context, goal *goalState, now int64) (protocol.PluginTransactionOperation, error) {
	entry, err := scope.value(ctx, "history")
	if err != nil {
		return protocol.PluginTransactionOperation{}, err
	}
	var history struct {
		Items []json.RawMessage `json:"items"`
	}
	if entry.Present {
		if err := json.Unmarshal(entry.Value, &history); err != nil {
			return protocol.PluginTransactionOperation{}, err
		}
	}
	if goal.FinishedMs != nil {
		now = *goal.FinishedMs
	}
	item, err := json.Marshal(map[string]any{"id": goal.ID, "objective": goal.Objective, "outcome": "completed",
		"turns": goal.Turns, "elapsed_ms": goal.ElapsedMs, "tokens": goal.Tokens, "evidence": goal.Evidence, "finished_ms": now})
	if err != nil {
		return protocol.PluginTransactionOperation{}, err
	}
	items := []json.RawMessage{item}
	for _, item := range history.Items {
		var previous struct{ ID string }
		if err := json.Unmarshal(item, &previous); err != nil {
			return protocol.PluginTransactionOperation{}, err
		}
		if previous.ID != goal.ID {
			items = append(items, item)
			if len(items) == 20 {
				break
			}
		}
	}
	history.Items = items
	return goalWriteOp(entry, history)
}

func goalWriteOp(entry protocol.PluginStorageConversationEntry, value any) (protocol.PluginTransactionOperation, error) {
	body, err := json.Marshal(value)
	return protocol.PluginTransactionOperation{Kind: "conversation_write", Data: &protocol.PluginTransactionOperationConversationWritePayload{
		Key: entry.Key, Value: body, ExpectedRevision: entry.Revision,
	}}, err
}

func (scope goalToolScope) write(ctx context.Context, call canonical.ToolRequest, entry protocol.PluginStorageConversationEntry, goal *goalState, ops []protocol.PluginTransactionOperation) error {
	if scope.invocation.Turn.Config.Mode == protocol.WorkModePlan {
		return goalToolFault(protocol.ErrorCodePermissionDenied, "planning mode permits read-only tools")
	}
	write, err := goalWriteOp(entry, goal)
	if err != nil {
		return err
	}
	request := scope.request("plugin_transaction", &protocol.CommandPluginTransactionPayload{Operations: append(ops, write)})
	alias := plugins.ToolAlias(scope.plugin.Package.Name, scope.name)
	identity := plugins.ToolCallIdentity(scope.invocation.Run.Turn, scope.plugin.Package, alias, call.ID)
	request.ID = plugins.ToolWriteIdentity(identity, 0)
	_, err = scope.node.dispatchCallbackResult(ctx, request)
	return err
}

func goalToolFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func goalResult(goal *goalState, revision uint64, limits goalToolLimits, now int64) map[string]any {
	state := goal.Status
	var claim, reason *string
	if goal.Claim != nil {
		claim = &goal.Claim.Note
	}
	switch state {
	case "active":
		state = "running"
		if claim != nil {
			state = "verifying"
		} else if goal.Waiting {
			state = "waiting"
		}
	case "paused":
		reason = goal.Reason
	}
	return map[string]any{
		"goal": map[string]any{
			"id": goal.ID, "revision": strconv.FormatUint(revision, 10), "objective": goal.Objective, "status": goal.Status, "state": state,
			"reason": reason, "detail": goal.Detail, "claim": claim, "evidence": goal.Evidence, "turns": goal.Turns,
			"automatic_turns": goal.Chain, "turn_limit": limits.Turns, "tokens": goal.Tokens, "token_budget": limits.Tokens,
			"elapsed_ms": goal.ElapsedMs + goal.openTime(now), "time_limit_ms": limits.Minutes * 60000,
		},
		"sailry_content": map[string]any{"version": 1, "blocks": []any{
			map[string]any{"kind": "notice", "message": goalToolLabels()[state]},
			map[string]any{"kind": "text", "path": "/goal/objective"},
		}},
	}
}
