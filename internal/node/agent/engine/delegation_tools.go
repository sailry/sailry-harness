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

const spawnSchema = `{"type":"object","additionalProperties":false,"required":["task"],"properties":{"session":{"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$","description":"Exact session ID of an idle child to continue; omit for a new child"},"role":{"type":["string","null"],"description":"Exact roster role key; omit or pass null to use this turn's model"},"task":{"type":"string","description":"Goal, scope, expected result and established findings"},"title":{"type":"string","minLength":1,"description":"Short single-line label for the child"},"background":{"type":"boolean","description":"Not owned by this turn; the result arrives as a later notice"},"overlap":{"type":"boolean","description":"Continue parent work while this foreground child runs. Defaults to true in Flagship. Set false if the next parent step needs its result."},"worktree":{"type":["string","null"],"description":"Registered worktree ID for a new child; defaults to this turn's worktree"}}}`
const listSubagentsSchema = `{"type":"object","additionalProperties":false,"properties":{"before":{"type":"string","pattern":"^[1-9][0-9]{0,18}$","description":"Use next_before from the preceding page"},"limit":{"type":"integer","minimum":1,"maximum":50,"default":20}}}`

func delegationAvailable(x *execution) bool {
	return store.DelegationAvailable(x.inv.Session, x.inv.Turn)
}

func (r *Runtime) usableRoles(x *execution) []protocol.RoleProfile {
	var usable []protocol.RoleProfile
	for _, role := range x.inv.Turn.Roles.Profiles {
		if r.usableRole(x, role) {
			usable = append(usable, role)
		}
	}
	return usable
}

func (r *Runtime) missingRoleSkills(x *execution, role protocol.RoleProfile) []string {
	var skills map[string]protocol.PluginSkill
	if x.resources != nil {
		skills = x.resources.Skills()
	}
	var missing []string
	for _, key := range role.Skills {
		if _, ok := skills[key]; !ok {
			missing = append(missing, key)
		}
	}
	slices.Sort(missing)
	return missing
}

func spawnDescriptionFor(x *execution, usable []protocol.RoleProfile) string {
	if len(usable) > 0 {
		return delegationDescriptions["spawn_agent"]
	}
	const paragraph1 = "Delegate a bounded multi-step task when authorized; handle tiny lookups or edits directly. Send independent spawns together. Foreground children in Flagship overlap parent work by default; set overlap=false only if the very next parent step immediately blocks on the result (outside Flagship, set overlap=true to opt in). Completed overlapping results arrive automatically—do not re-fetch an un-truncated completion with wait_agent. Detached background children complete as later notices. Extra children queue at the concurrency limit. Keep the returned session and turn IDs."
	roleSentence := "No team roles are configured for this turn; omit role so new children run with this turn's model."
	if len(x.inv.Turn.Roles.Profiles) > 0 {
		roleSentence = "No roles from the captured team are available for new children in this turn; omit role to use this turn's model."
		if strings.TrimSpace(x.inv.Turn.Roles.Instructions) != "" {
			roleSentence = "No roles from the captured team are available for new children in this turn; follow the team instructions and omit role when roleless delegation is allowed."
		}
	}
	paragraph2 := "Give each child its goal, disjoint scope, known path:line facts, expected output, and stop condition. " +
		roleSentence +
		" Children inherit the worktree, mode, model settings, and permissions and cannot delegate. Use guide_agent for follow-ups; to continue an idle child with spawn_agent, pass its exact session ID and omit role. Do not restart user-stopped or cancelled children unless asked."
	return paragraph1 + "\n\n" + paragraph2
}

func (r *Runtime) delegationTools(x *execution) []Tool {
	if !delegationAvailable(x) {
		return nil
	}
	usable := r.usableRoles(x)
	deferred := x.inv.Turn.Config.Mode != protocol.WorkModeFlagship && selectedRole(x.inv.Message) == nil
	var values []Tool
	for _, name := range []string{"spawn_agent", "guide_agent", "wait_agent"} {
		description := delegationDescriptions[name]
		var schema string
		switch name {
		case "spawn_agent":
			description = spawnDescriptionFor(x, usable)
			schema = spawnSchema
		case "guide_agent":
			schema = `{"type":"object","properties":{"session":{"type":"string"},"message":{"type":"string","minLength":1},"overlap":{"type":"boolean","description":"Continue parent work while an idle child runs its new task. Defaults to true in Flagship. Guidance to an already running child returns immediately."}},"required":["session","message"],"additionalProperties":false}`
		default:
			schema = `{"type":"object","properties":{"session":{"type":"string"},"turn":{"type":"string","description":"Exact execution from list_subagents or a completion notice; omit for the current execution"}},"required":["session"],"additionalProperties":false}`
		}
		observe := name == "wait_agent"
		tool := jsonTool(name, description, schema, observe, observe, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
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
		tool.Deferred = deferred
		values = append(values, tool)
	}
	list := jsonTool("list_subagents", delegationDescriptions["list_subagents"], listSubagentsSchema, true, true, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
		if ctx.Err() != nil || x.ctx.Err() != nil {
			return invalidTool("delegation is no longer active")
		}
		var options store.SubagentListOptions
		if err := protocol.DecodeObject(call.Arguments, &options, nil, true); err != nil {
			return invalidTool("invalid subagent list arguments")
		}
		var page store.SubagentPage
		if err := r.db.Read(ctx, func(db *sql.Conn) error {
			var e error
			page, e = store.ListSubagents(ctx, db, x.inv.Run.Turn, options)
			return e
		}); err != nil {
			return toolFault(err)
		}
		return ToolResult{Value: page}
	})
	list.Deferred = deferred
	list.SearchLabels = []string{"子智能体", "子智能體", "子代理", "查看", "续接", "恢复"}
	stop := jsonTool("stop_agent", delegationDescriptions["stop_agent"], `{"type":"object","properties":{"session":{"type":"string","description":"Exact session ID of the child to stop"}},"required":["session"],"additionalProperties":false}`, false, false, func(ctx context.Context, call canonical.ToolRequest) ToolResult {
		if ctx.Err() != nil || x.ctx.Err() != nil {
			return invalidTool("delegation is no longer active")
		}
		return r.stopChild(x, call)
	})
	stop.Deferred = deferred
	return append(values, list, stop)
}

// stopChild stops a child this conversation owns through the controller's
// explicit stop path, then observes it settle so its cancelled record is
// collected with this result instead of reopening a reply.
func (r *Runtime) stopChild(x *execution, call canonical.ToolRequest) ToolResult {
	var args struct {
		Session protocol.SessionID `json:"session"`
	}
	if json.Unmarshal(call.Arguments, &args) != nil {
		return invalidTool("invalid child control arguments")
	}
	unlock := r.LockPublication()
	var run protocol.ConversationRun
	err := r.db.Write(x.ctx, func(db *sql.Conn) error {
		var events []protocol.Event
		var err error
		run, events, err = store.StopChild(x.ctx, db, x.inv.Run.Turn, args.Session, r.clock.Now())
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(x.ctx, db, r.node, events)
		return err
	})
	if err == nil {
		r.StopCommitted(run)
		r.publish()
		r.feed.Publish(run.Session, protocol.ConversationChange{Kind: "run", Data: run})
		r.RefreshCommitted(run.Session)
		r.RefreshCommitted(x.inv.Session.ID, run.Turn)
	}
	unlock()
	if err != nil {
		return toolFault(err)
	}
	r.Wake()
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
		if err := r.db.Read(x.ctx, func(db *sql.Conn) error {
			var err error
			state, err = store.ReadChildState(x.ctx, db, x.inv.Run.Turn, args.Session, &run.Turn)
			return err
		}); err != nil {
			return toolFault(err)
		}
		value := map[string]any{"session": state.Run.Session, "turn": state.Run.Turn, "status": state.Run.Status}
		if !childPending(state.Run) {
			return ToolResult{Value: value, Collect: &collectedChild{session: args.Session, turn: state.Run.Turn}}
		}
		select {
		case <-x.ctx.Done():
			return toolFault(x.ctx.Err())
		case <-deadline:
			// Its cancelled result arrives automatically once it settles.
			return ToolResult{Value: value}
		case <-changed:
		}
	}
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

func (r *Runtime) delegationRoster(x *execution, state store.AgentState) string {
	if !delegationAvailable(x) {
		return ""
	}
	var roster []string
	var unavailable []string
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
		} else {
			unavailable = append(unavailable, fmt.Sprintf("%s (missing: %s)", role.Key, strings.Join(r.missingRoleSkills(x, role), ", ")))
		}
	}
	hasTeamInstructions := strings.TrimSpace(x.inv.Turn.Roles.Instructions) != ""
	text := "Omit role to create children using this turn's model."
	if len(roster) > 0 {
		if hasTeamInstructions {
			text = "Follow the team instructions and choose the exact key of the role whose responsibilities fit. Available roles:\n" + strings.Join(roster, "\n") + "\nIf the team instructions allow delegation without a role, omit role to use this turn's model."
		} else {
			text = "Available roles (pass the exact key when a role's responsibilities fit, or omit role to use this turn's model):\n" + strings.Join(roster, "\n")
		}
	} else if len(x.inv.Turn.Roles.Profiles) > 0 {
		if hasTeamInstructions {
			text = "The captured team has no available roles for new children. Follow the team instructions for work outside role responsibilities."
		} else {
			text = "The captured team has no available roles for new children; omit role to use this turn's model."
		}
	}
	if len(unavailable) > 0 {
		text += "\nUnavailable roles (missing required skills): " + strings.Join(unavailable, ", ")
	}
	if list := findTool(x.tools, "list_subagents"); list != nil {
		text += "\nBefore reusing earlier children across turns, inspect list_subagents for exact IDs and status; never resume a cancelled or stopped child unless explicitly requested."
		if list.Deferred && !slices.Contains(state.LoadedTools, "list_subagents") {
			text += "\nLoad it with tool_search using select:list_subagents."
		}
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
		Session    *protocol.SessionID `json:"session"`
		Role       *string             `json:"role"`
		Background bool                `json:"background"`
		Overlap    *bool               `json:"overlap"`
	}
	if json.Unmarshal(arguments, &args) != nil {
		return invalidTool("invalid delegation arguments")
	}
	if args.Role != nil && strings.TrimSpace(*args.Role) == "" {
		if args.Session != nil {
			return invalidTool("omit role when continuing an existing child session")
		}
		var fields map[string]json.RawMessage
		if json.Unmarshal(arguments, &fields) != nil {
			return invalidTool("invalid delegation arguments")
		}
		delete(fields, "role")
		normalized, err := json.Marshal(fields)
		if err != nil {
			return invalidTool("invalid delegation arguments")
		}
		arguments = normalized
		args.Role = nil
	}
	if args.Role != nil {
		usable := r.usableRoles(x)
		if !slices.ContainsFunc(usable, func(role protocol.RoleProfile) bool { return role.Key == *args.Role }) {
			if index := slices.IndexFunc(x.inv.Turn.Roles.Profiles, func(role protocol.RoleProfile) bool { return role.Key == *args.Role }); index >= 0 {
				missing := r.missingRoleSkills(x, x.inv.Turn.Roles.Profiles[index])
				return invalidTool(fmt.Sprintf("role %q is unavailable in this turn because required skills are missing: %s", *args.Role, strings.Join(missing, ", ")))
			}
			if len(usable) == 0 {
				return invalidTool("this turn has no available roles; omit role to use this turn's model")
			}
			keys := make([]string, len(usable))
			for i, role := range usable {
				keys[i] = role.Key
			}
			return invalidTool(fmt.Sprintf("role %q is not listed in this turn's roster; available roles: %s (or omit role to use this turn's model)", *args.Role, strings.Join(keys, ", ")))
		}
	}
	explicitOverlap := args.Overlap != nil && *args.Overlap
	if args.Background && explicitOverlap {
		return invalidTool("overlap requires a foreground child")
	}
	selectedCall := call.ID == "sailry-selected-"+x.inv.Run.Turn.String()
	defaultOverlap := args.Overlap == nil && call.Name == "spawn_agent" && !selectedCall && x.inv.Turn.Config.Mode == protocol.WorkModeFlagship
	mode := childForeground
	if args.Background {
		mode = childBackground
	} else if explicitOverlap || defaultOverlap {
		mode = childOverlap
	}
	var selected *protocol.ConversationInput
	if selectedCall {
		copy := x.inv.Message
		copy.References = slices.DeleteFunc(slices.Clone(copy.References), func(ref protocol.ConversationReference) bool { return ref.Target.Kind == "agent" })
		if strings.TrimSpace(copy.Text) == "" && len(copy.References) == 0 && len(copy.Attachments) == 0 {
			copy.Text = "Complete the request using the selected context"
		}
		selected = &copy
	}
	return r.admitChild(x, arguments, mode, func(ctx context.Context, db *sql.Conn) (*store.AgentInvocation, []protocol.Event, error) {
		inv, events, err := store.DelegatePrepared(ctx, db, r.node, x.inv.Run.Turn, toolIdentity(call), arguments, selected, args.Background, mode == childOverlap, r.clock.Now())
		return &inv, events, err
	})
}

// guideChild guides or resumes a child for call. arguments are the ones that
// execute: the call's own, or those a package tool prepared from them.
func (r *Runtime) guideChild(x *execution, call canonical.ToolRequest, arguments json.RawMessage) ToolResult {
	var args struct {
		Overlap *bool `json:"overlap"`
	}
	if json.Unmarshal(arguments, &args) != nil {
		return invalidTool("invalid child guidance arguments")
	}
	// Like spawn_agent, only the core tool overlaps by default; a package tool
	// bound to agents.guide keeps the foreground wait it was written for.
	overlap := call.Name == "guide_agent" && x.inv.Turn.Config.Mode == protocol.WorkModeFlagship
	if args.Overlap != nil {
		overlap = *args.Overlap
	}
	mode := childForeground
	if overlap {
		mode = childOverlap
	}
	var state store.ChildState
	result := r.admitChild(x, arguments, mode, func(ctx context.Context, db *sql.Conn) (*store.AgentInvocation, []protocol.Event, error) {
		var inv *store.AgentInvocation
		var events []protocol.Event
		var err error
		state, inv, events, err = store.GuideChild(ctx, db, r.node, x.inv.Run.Turn, toolIdentity(call), arguments, mode == childOverlap, r.clock.Now())
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
			value := map[string]any{"session": state.Run.Session, "turn": state.Run.Turn, "status": state.Run.Status, "response": state.Response, "error": state.Run.Error}
			if state.TurnLimit {
				value["turn_limit"] = store.ChildTurnLimitSummary
			}
			var meta *map[string]json.RawMessage
			if len(state.FileRevisions) > 0 {
				if encoded, err := json.Marshal(state.FileRevisions); err == nil {
					entries := map[string]json.RawMessage{canonical.FileChangesKey(state.Run.Worktree): encoded}
					meta = &entries
				}
			}
			return ToolResult{Value: value, Meta: meta, Collect: &collectedChild{session: args.Session, turn: state.Run.Turn}}
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
		if state.Run.Status == protocol.ConversationStatusQueued && !state.Ready {
			// A foreground admission in this batch can only start after the
			// model has a durable response for the call that created it.
			value["admitted"], value["pending"] = true, true
			value["hint"] = "Starts after this tool batch; its result arrives automatically, or wait_agent can collect it in a later step."
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
