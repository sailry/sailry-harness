package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const spawnSchema = `{"type":"object","additionalProperties":false,"required":["task"],"properties":{"session":{"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"},"role":{"type":["string","null"]},"task":{"type":"string"},"title":{"type":"string","minLength":1},"background":{"type":"boolean"},"worktree":{"type":["string","null"]}}}`

func delegationAvailable(x *execution) bool {
	return x.inv.Session.Delegation == nil && x.inv.Turn.Config.Resource == nil && x.inv.Turn.Roles.Execution == protocol.ConversationExecutionTeam
}

func (r *Runtime) delegationTools(x *execution) []Tool {
	if !delegationAvailable(x) {
		return nil
	}
	var values []Tool
	for _, name := range []string{"spawn_agent", "guide_agent", "wait_agent"} {
		schema := spawnSchema
		if name == "guide_agent" {
			schema = `{"type":"object","properties":{"session":{"type":"string"},"message":{"type":"string","minLength":1}},"required":["session","message"],"additionalProperties":false}`
		}
		if name == "wait_agent" {
			schema = `{"type":"object","properties":{"session":{"type":"string"},"turn":{"type":"string","description":"Exact execution from a completion notice; omit for the latest execution"}},"required":["session"],"additionalProperties":false}`
		}
		tool := jsonTool(name, delegationDescriptions[name], schema, false, false, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
			if ctx.Err() != nil || x.ctx.Err() != nil {
				return invalidTool("delegation is no longer active")
			}
			switch name {
			case "spawn_agent":
				return r.spawnChild(x, call, call.Arguments)
			case "guide_agent":
				return r.guideChild(x, call, call.Arguments)
			default:
				return r.waitChild(x, call)
			}
		})
		if name != "wait_agent" {
			tool.ExecutePrepared = func(ctx context.Context, call canonical.ToolRequest, arguments json.RawMessage) ToolResult {
				if ctx.Err() != nil || x.ctx.Err() != nil {
					return invalidTool("delegation is no longer active")
				}
				if name == "guide_agent" {
					return r.guideChild(x, call, arguments)
				}
				return r.spawnChild(x, call, arguments)
			}
		}
		tool.Delegation = true
		tool.Deferred = x.inv.Turn.Config.Mode != protocol.WorkModeFlagship && selectedRole(x.inv.Message) == nil
		values = append(values, tool)
	}
	return values
}

func selectedRole(input protocol.ConversationInput) *protocol.RoleReference {
	for _, ref := range input.References {
		if ref.Target.Kind == "agent" {
			raw, _ := json.Marshal(ref.Target.Data)
			var role protocol.RoleReference
			if json.Unmarshal(raw, &role) == nil {
				return &role
			}
		}
	}
	return nil
}

func (r *Runtime) selectedAgent(x *execution, state store.AgentState) (bool, error) {
	selected := selectedRole(x.inv.Message)
	if selected == nil {
		return false, nil
	}
	if !delegationAvailable(x) {
		return false, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "selected agent is unavailable in this turn"}
	}
	id := "sailry-selected-" + x.inv.Run.Turn.String()
	for _, record := range state.Records {
		if record.Turn != x.inv.Run.Turn {
			continue
		}
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if call, ok := block.Data.(*canonical.ToolRequest); ok && call.ID == id {
					return false, nil
				}
			}
		}
	}
	role := slices.IndexFunc(x.inv.Turn.Roles.Profiles, func(role protocol.RoleProfile) bool {
		return role.ID == selected.ID && role.Revision == selected.Revision && r.usableRole(x, role)
	})
	if role < 0 {
		return false, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "selected role is absent from the frozen roster"}
	}
	task := x.inv.Message.Text
	if strings.TrimSpace(task) == "" {
		task = "Complete the request using the selected context"
	}
	arguments, _ := json.Marshal(map[string]any{"role": x.inv.Turn.Roles.Profiles[role].Key, "task": task})
	record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "spawn_agent", Arguments: arguments}}), r.clock.Now())
	record.Provenance.Author = "assistant"
	return true, r.save(x.ctx, x.inv.Session.ID, record)
}

func (r *Runtime) usableRole(x *execution, role protocol.RoleProfile) bool {
	if x.resources == nil {
		return len(role.Skills) == 0
	}
	skills := x.resources.Skills()
	for _, key := range role.Skills {
		if _, ok := skills[key]; !ok {
			return false
		}
	}
	return true
}

func (r *Runtime) delegationContext(x *execution, state store.AgentState) string {
	text := r.delegationRoster(x, state)
	// Catalog changes never rebase the captured turn; application names the
	// exact conversation revision it replaces.
	if findTool(x.tools, "apply_agents") != nil || findTool(x.tools, "apply_team") != nil {
		if text != "" {
			text += "\n"
		}
		text += fmt.Sprintf("Captured conversation revision: %d. Catalog changes do not change this turn's roster. Explicit application affects future admission only; use the exact inspected revision and never silently rebase a conflict.", x.inv.Turn.Revision)
	}
	return text
}

func (r *Runtime) delegationRoster(x *execution, state store.AgentState) string {
	if !delegationAvailable(x) {
		return ""
	}
	var roster []string
	for _, role := range x.inv.Turn.Roles.Profiles {
		if r.usableRole(x, role) {
			line := role.Key + " (" + role.Name + ")"
			if role.ReadOnly {
				line += " [read only]"
			}
			if purpose := strings.TrimSpace(role.Description); purpose != "" {
				line += ": " + purpose
			}
			roster = append(roster, line)
		}
	}
	text := "No delegation roles are configured"
	if len(roster) > 0 {
		text = "Configured delegation roles:\n" + strings.Join(roster, "\n")
	}
	tool := findTool(x.tools, "spawn_agent")
	if tool != nil && tool.Deferred && !slices.Contains(state.LoadedTools, "spawn_agent") {
		return text + "\nLoad spawn_agent with tool_search to delegate."
	}
	if x.childBudget >= uint64(^uint32(0)) {
		return text + "\nConcurrency budget: no limit on children running at once in this conversation."
	}
	return text + fmt.Sprintf("\nConcurrency budget: up to %d children run at once in this conversation; later children queue in FIFO order.", x.childBudget)
}

// spawnChild admits a child for call. arguments are the ones that execute:
// the call's own, or those a package tool prepared from them.
func (r *Runtime) spawnChild(x *execution, call canonical.ToolRequest, arguments json.RawMessage) ToolResult {
	var args struct {
		Role       *string `json:"role"`
		Background bool    `json:"background"`
	}
	if json.Unmarshal(arguments, &args) != nil {
		return invalidTool("invalid delegation arguments")
	}
	if args.Role != nil && !slices.ContainsFunc(x.inv.Turn.Roles.Profiles, func(role protocol.RoleProfile) bool { return role.Key == *args.Role && r.usableRole(x, role) }) {
		return invalidTool("role is not listed in this turn's roster")
	}
	var selected *protocol.ConversationInput
	if call.ID == "sailry-selected-"+x.inv.Run.Turn.String() {
		copy := x.inv.Message
		copy.References = slices.DeleteFunc(slices.Clone(copy.References), func(ref protocol.ConversationReference) bool { return ref.Target.Kind == "agent" })
		if strings.TrimSpace(copy.Text) == "" && len(copy.References) == 0 && len(copy.Attachments) == 0 {
			copy.Text = "Complete the request using the selected context"
		}
		selected = &copy
	}
	return r.admitChild(x, arguments, args.Background, func(ctx context.Context, db *sql.Conn) (*store.AgentInvocation, []protocol.Event, error) {
		inv, events, err := store.DelegatePrepared(ctx, db, r.node, x.inv.Run.Turn, toolIdentity(call), arguments, selected, args.Background, r.clock.Now())
		return &inv, events, err
	})
}

// guideChild guides or resumes a child for call. arguments are the ones that
// execute: the call's own, or those a package tool prepared from them.
func (r *Runtime) guideChild(x *execution, call canonical.ToolRequest, arguments json.RawMessage) ToolResult {
	var state store.ChildState
	result := r.admitChild(x, arguments, false, func(ctx context.Context, db *sql.Conn) (*store.AgentInvocation, []protocol.Event, error) {
		var inv *store.AgentInvocation
		var events []protocol.Event
		var err error
		state, inv, events, err = store.GuideChild(ctx, db, r.node, x.inv.Run.Turn, toolIdentity(call), arguments, r.clock.Now())
		return inv, events, err
	})
	if result.Value == nil && result.Failure == nil {
		result.Value = map[string]any{"session": state.Run.Session, "turn": state.Run.Turn, "status": "queued"}
	}
	if value, ok := result.Value.(map[string]any); ok && state.Recipient != nil {
		value["recipient"] = *state.Recipient
	}
	return result
}

func (r *Runtime) waitChild(x *execution, call canonical.ToolRequest) ToolResult {
	var args struct {
		Session protocol.SessionID `json:"session"`
		Turn    *protocol.TurnID   `json:"turn"`
	}
	if json.Unmarshal(call.Arguments, &args) != nil {
		return invalidTool("invalid child control arguments")
	}
	window, cancel := context.WithCancel(x.ctx)
	defer cancel()
	deadline := make(chan struct{})
	go func() {
		if r.clock.Sleep(window, 150*time.Second) == nil {
			close(deadline)
		}
	}()
	for {
		changed := r.activitySignal()
		var state store.ChildState
		var guidance bool
		err := r.db.Read(x.ctx, func(db *sql.Conn) error {
			var err error
			state, err = store.ReadChildState(x.ctx, db, x.inv.Run.Turn, args.Session, args.Turn)
			if err != nil {
				return err
			}
			guidance, err = store.PendingGuidance(x.ctx, db, x.inv.Run.Turn)
			return err
		})
		if err != nil {
			return toolFault(err)
		}
		if !childPending(state.Run) {
			return ToolResult{Value: map[string]any{"session": state.Run.Session, "turn": state.Run.Turn, "status": state.Run.Status, "response": state.Response, "error": state.Run.Error}, Collect: &collectedChild{session: args.Session, turn: state.Run.Turn}}
		}
		value := map[string]any{"session": state.Run.Session, "turn": state.Run.Turn, "status": state.Run.Status}
		if state.Paused && state.Run.Status == protocol.ConversationStatusQueued {
			value["paused"] = true
			return ToolResult{Value: value}
		}
		if guidance {
			value["guidance_pending"] = true
			return ToolResult{Value: value}
		}
		select {
		case <-x.ctx.Done():
			return toolFault(x.ctx.Err())
		case <-deadline:
			value["admitted"], value["pending"] = true, true
			return ToolResult{Value: value}
		case <-changed:
		}
	}
}

func childActive(status protocol.ConversationStatus) bool {
	return status == protocol.ConversationStatusQueued || status == protocol.ConversationStatusRunning || status == protocol.ConversationStatusStopping
}

// childPending reports whether a child's run is still to settle. A queued
// child stopped by a controller is cancelled first and finished by the next
// claim pass; its result is collected only once its record is closed.
func childPending(run protocol.ConversationRun) bool {
	return childActive(run.Status) || run.Status == protocol.ConversationStatusCancelled && run.FinishedMs == nil
}
