package node

import (
	"context"
	"encoding/json"
	"math"
	"slices"
	"strconv"

	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const catalogLost = "catalog response is unavailable"

var (
	agentFields = []string{"key", "name", "instructions", "description", "read_only", "model", "max_turns", "skills", "appearance"}
	teamFields  = []string{"key", "name", "description", "instructions", "model", "members", "appearance"}
)

// catalogRevision reads an exact positive decimal-string revision.
func catalogRevision(value json.RawMessage) (uint64, bool) {
	var text string
	if value == nil || json.Unmarshal(value, &text) != nil {
		return 0, false
	}
	number, err := strconv.ParseUint(text, 10, 64)
	return number, err == nil && number > 0 && strconv.FormatUint(number, 10) == text && number <= math.MaxInt64
}

// catalogInput accepts an object holding only allowed keys and every required one.
func catalogInput(arguments json.RawMessage, allowed []string, required ...string) (map[string]json.RawMessage, bool) {
	var input map[string]json.RawMessage
	if json.Unmarshal(arguments, &input) != nil || input == nil {
		return nil, false
	}
	for key := range input {
		if !slices.Contains(allowed, key) {
			return nil, false
		}
	}
	for _, key := range required {
		if _, present := input[key]; !present {
			return nil, false
		}
	}
	return input, true
}

// catalogMerge applies the supplied definition fields over a base entry.
func catalogMerge(base any, input map[string]json.RawMessage, fields []string, target any) error {
	body, err := json.Marshal(base)
	if err != nil {
		return err
	}
	var merged map[string]json.RawMessage
	if err := json.Unmarshal(body, &merged); err != nil {
		return err
	}
	for _, key := range fields {
		if value, present := input[key]; present {
			merged[key] = value
		}
	}
	if body, err = json.Marshal(merged); err != nil {
		return err
	}
	if json.Unmarshal(body, target) != nil {
		return connectionFault(protocol.ErrorCodeInvalidRequest, "invalid catalog arguments")
	}
	return nil
}

func catalogIdentity(input map[string]json.RawMessage, target any) error {
	if json.Unmarshal(input["id"], target) != nil {
		return connectionFault(protocol.ErrorCodeInvalidRequest, "invalid catalog ID")
	}
	return nil
}

// catalogTools declares the role and team catalog tools. Definitions come from
// and return to the catalog the settings pages edit; application targets only
// this conversation, for future turn admission.
func (node *Node) catalogTools(inv *store.AgentInvocation) []agentTool {
	invalid := func(message string) error { return connectionFault(protocol.ErrorCodeInvalidRequest, message) }
	changed := connectionFault(protocol.ErrorCodeRevisionConflict, "catalog entry changed")
	revision := map[string]any{"type": "string", "pattern": "^[1-9][0-9]*$"}
	text := map[string]any{"type": "string"}
	object := func(required []any, properties map[string]any) map[string]any {
		return map[string]any{"type": "object", "additionalProperties": false, "required": required, "properties": properties}
	}
	empty := func() map[string]any {
		return map[string]any{"type": "object", "additionalProperties": false, "properties": map[string]any{}}
	}
	nullable := map[string]any{"type": []any{"object", "null"}}
	agent := func(identity bool) map[string]any {
		properties := map[string]any{"key": text, "name": text, "instructions": text, "description": text, "read_only": map[string]any{"type": "boolean"},
			"model": nullable, "max_turns": map[string]any{"type": []any{"integer", "null"}, "minimum": 1}, "skills": map[string]any{"type": "array", "items": text}, "appearance": nullable}
		if identity {
			properties["id"], properties["expected_revision"] = text, revision
		}
		return properties
	}
	team := func(identity bool) map[string]any {
		properties := map[string]any{"key": text, "name": text, "description": text, "instructions": text, "model": nullable, "members": map[string]any{"type": "array", "items": text}, "appearance": nullable}
		if identity {
			properties["id"], properties["expected_revision"] = text, revision
		}
		return properties
	}
	reference := object([]any{"id", "revision"}, map[string]any{"id": text, "revision": revision})
	selection := object([]any{"id", "revision"}, map[string]any{"id": text, "revision": revision})
	selection["type"] = []any{"object", "null"}
	identity := []any{"id", "expected_revision"}

	// edit reads the changed fields and exact revision of one catalog entry.
	edit := func(arguments json.RawMessage, fields []string, message string) (map[string]json.RawMessage, uint64, error) {
		input, valid := catalogInput(arguments, append(slices.Clone(fields), "id", "expected_revision"), "id", "expected_revision")
		expected, exact := catalogRevision(input["expected_revision"])
		if !valid || !exact || !slices.ContainsFunc(fields, func(key string) bool { _, present := input[key]; return present }) {
			return nil, 0, invalid(message)
		}
		return input, expected, nil
	}
	removal := func(arguments json.RawMessage, message string) (map[string]json.RawMessage, uint64, error) {
		input, valid := catalogInput(arguments, []string{"id", "expected_revision"}, "id", "expected_revision")
		expected, exact := catalogRevision(input["expected_revision"])
		if !valid || !exact {
			return nil, 0, invalid(message)
		}
		return input, expected, nil
	}
	list := func(kind string) func(context.Context, json.RawMessage) (protocol.Command, error) {
		return func(_ context.Context, arguments json.RawMessage) (protocol.Command, error) {
			if _, valid := catalogInput(arguments, nil); !valid {
				return protocol.Command{}, invalid("Invalid catalog arguments")
			}
			return protocol.Command{Kind: kind}, nil
		}
	}
	exact := func(entry any, revision uint64) (any, error) { return operations.ExactRevision(entry, revision) }
	agents := func(roles []protocol.RoleProfile) ([]any, error) {
		values := make([]any, 0, len(roles))
		for _, role := range roles {
			value, err := exact(role, role.Revision)
			if err != nil {
				return nil, err
			}
			values = append(values, value)
		}
		return values, nil
	}
	teams := func(list []protocol.TeamProfile) ([]any, error) {
		values := make([]any, 0, len(list))
		for _, team := range list {
			value, err := exact(team, team.Revision)
			if err != nil {
				return nil, err
			}
			values = append(values, value)
		}
		return values, nil
	}
	unavailable := connectionFault(protocol.ErrorCodeInternal, "catalog outcome is unavailable")
	// Public definitions only; frozen provider configuration never leaves the store.
	listedAgents := func(wrapped bool) func(context.Context, protocol.Output) (any, error) {
		return func(_ context.Context, output protocol.Output) (any, error) {
			roles, listed := dispatchPayload[[]protocol.RoleProfile](output.Data)
			if !listed || output.Kind != "roles" {
				return nil, unavailable
			}
			values, err := agents(roles)
			if err != nil || !wrapped {
				return values, err
			}
			return map[string]any{"agents": values}, nil
		}
	}
	listedTeams := func(wrapped bool) func(context.Context, protocol.Output) (any, error) {
		return func(_ context.Context, output protocol.Output) (any, error) {
			list, listed := dispatchPayload[[]protocol.TeamProfile](output.Data)
			if !listed || output.Kind != "teams" {
				return nil, unavailable
			}
			values, err := teams(list)
			if err != nil || !wrapped {
				return values, err
			}
			return map[string]any{"teams": values}, nil
		}
	}
	savedAgent := func(_ context.Context, output protocol.Output) (any, error) {
		role, saved := dispatchPayload[protocol.RoleProfile](output.Data)
		if !saved || output.Kind != "role" {
			return nil, unavailable
		}
		return exact(role, role.Revision)
	}
	savedTeam := func(_ context.Context, output protocol.Output) (any, error) {
		team, saved := dispatchPayload[protocol.TeamProfile](output.Data)
		if !saved || output.Kind != "team" {
			return nil, unavailable
		}
		return exact(team, team.Revision)
	}
	applied := func(selected bool) func(context.Context, protocol.Output) (any, error) {
		return func(_ context.Context, output protocol.Output) (any, error) {
			session, updated := dispatchPayload[protocol.Session](output.Data)
			if !updated || output.Kind != "session" {
				return nil, unavailable
			}
			roles := make([]any, 0, len(session.Roles.Profiles))
			for _, role := range session.Roles.Profiles {
				roles = append(roles, map[string]any{"id": role.ID, "revision": strconv.FormatUint(role.Revision, 10)})
			}
			receipt := map[string]any{"session": session.ID, "revision": strconv.FormatUint(session.Revision, 10), "roles": roles}
			if selected {
				receipt["team"] = nil
				if team := session.Roles.Team; team != nil {
					receipt["team"] = map[string]any{"id": team.ID, "revision": strconv.FormatUint(team.Revision, 10)}
				}
			}
			return receipt, nil
		}
	}

	declared := []agentTool{
		{
			name:        "list_agents",
			description: "List public agent definitions and exact revisions in this execution Node's catalog. Catalog edits affect new conversations only until explicitly applied; they never rewrite this turn's roster or existing children. No provider configuration or credentials are returned.",
			parameters:  empty(), readOnly: true, prepare: list("list_roles"), project: listedAgents(true),
		},
		{
			name:        "add_agent",
			description: "Add an agent definition to this execution Node's catalog. Use a unique lowercase key, display name and instructions. Optional model identifies an existing provider/model, never credentials. This does not change this conversation's roster; use apply_agents only when explicitly requested.",
			parameters:  object([]any{"key", "name", "instructions"}, agent(false)),
			prepare: func(_ context.Context, arguments json.RawMessage) (protocol.Command, error) {
				input, valid := catalogInput(arguments, agentFields, "key", "name", "instructions")
				if !valid {
					return protocol.Command{}, invalid("Invalid agent definition")
				}
				base := map[string]any{"id": protocol.NewRoleID(), "revision": 0, "key": "", "name": "", "description": "", "instructions": "",
					"read_only": false, "model": nil, "max_turns": nil, "skills": []string{}, "appearance": nil}
				var role protocol.RoleProfile
				if err := catalogMerge(base, input, agentFields, &role); err != nil {
					return protocol.Command{}, err
				}
				// An invalid definition is refused before its approval is requested.
				if err := store.ValidateRole(role); err != nil {
					return protocol.Command{}, err
				}
				return protocol.Command{Kind: "put_role", Data: &protocol.CommandPutRolePayload{Role: role}}, nil
			},
			project: savedAgent,
		},
		{
			name:        "edit_agent",
			description: "Edit one catalog agent using its exact ID and decimal-string revision from list_agents. Omitted fields are preserved; explicit null clears optional fields. Inspect conflicts instead of silently rebasing. Existing conversations, queued turns and children keep their captures until explicit application for future admission.",
			parameters:  object(identity, agent(true)),
			prepare: func(ctx context.Context, arguments json.RawMessage) (protocol.Command, error) {
				output, err := node.agentCommand(ctx, inv, protocol.Command{Kind: "list_roles"}, catalogLost)
				if err != nil {
					return protocol.Command{}, err
				}
				input, expected, err := edit(arguments, agentFields, "Inspect the agent and use its exact revision")
				if err != nil {
					return protocol.Command{}, err
				}
				var id protocol.RoleID
				if err := catalogIdentity(input, &id); err != nil {
					return protocol.Command{}, err
				}
				roles, listed := dispatchPayload[[]protocol.RoleProfile](output.Data)
				if !listed {
					return protocol.Command{}, connectionFault(protocol.ErrorCodeInternal, "agent catalog is unavailable")
				}
				index := slices.IndexFunc(roles, func(role protocol.RoleProfile) bool { return role.ID == id })
				if index < 0 {
					return protocol.Command{}, connectionFault(protocol.ErrorCodeNotFound, "Agent not found")
				}
				if roles[index].Revision != expected {
					return protocol.Command{}, changed
				}
				var role protocol.RoleProfile
				if err := catalogMerge(roles[index], input, agentFields, &role); err != nil {
					return protocol.Command{}, err
				}
				if err := store.ValidateRole(role); err != nil {
					return protocol.Command{}, err
				}
				return protocol.Command{Kind: "put_role", Data: &protocol.CommandPutRolePayload{Role: role, ExpectedRevision: expected}}, nil
			},
			project: savedAgent,
		},
		{
			name:        "delete_agent",
			description: "Remove exactly one agent definition from this execution Node's catalog using its ID and decimal-string revision from list_agents. Confirm the user's intent before deletion. Existing conversations, queued turns and children retain their immutable definitions and history; this does not delete any conversation or automatically change its roster.",
			parameters:  object(identity, map[string]any{"id": text, "expected_revision": revision}),
			prepare: func(_ context.Context, arguments json.RawMessage) (protocol.Command, error) {
				input, expected, err := removal(arguments, "Inspect the agent and use its exact revision")
				if err != nil {
					return protocol.Command{}, err
				}
				var id protocol.RoleID
				if err := catalogIdentity(input, &id); err != nil {
					return protocol.Command{}, err
				}
				return protocol.Command{Kind: "remove_role", Data: &protocol.CommandRemoveRolePayload{Role: id, ExpectedRevision: expected}}, nil
			},
			project: listedAgents(false),
		},
		{
			name:        "apply_agents",
			description: "Explicitly replace this conversation's selected agent roster for future turn admission only. Supply the complete desired list of catalog IDs with exact decimal-string revisions, and this conversation's exact expected_session_revision from initialization or a completed apply_agents receipt. An empty list clears future role selection. Never silently rebase conflicts. This running turn, already queued turns and existing children keep their immutable captures; this cannot target another conversation.",
			parameters: object([]any{"agents", "expected_session_revision"}, map[string]any{
				"agents": map[string]any{"type": "array", "items": reference}, "expected_session_revision": revision}),
			prepare: func(_ context.Context, arguments json.RawMessage) (protocol.Command, error) {
				rejected := invalid("Use distinct agents and exact revisions")
				input, valid := catalogInput(arguments, []string{"agents", "expected_session_revision"}, "agents", "expected_session_revision")
				expected, exact := catalogRevision(input["expected_session_revision"])
				var entries []json.RawMessage
				if !valid || !exact || json.Unmarshal(input["agents"], &entries) != nil || entries == nil {
					return protocol.Command{}, rejected
				}
				roles := make([]protocol.RoleReference, 0, len(entries))
				for _, entry := range entries {
					fields, valid := catalogInput(entry, []string{"id", "revision"}, "id", "revision")
					revision, exact := catalogRevision(fields["revision"])
					var id protocol.RoleID
					if !valid || !exact || catalogIdentity(fields, &id) != nil ||
						slices.ContainsFunc(roles, func(role protocol.RoleReference) bool { return role.ID == id }) {
						return protocol.Command{}, rejected
					}
					roles = append(roles, protocol.RoleReference{ID: id, Revision: revision})
				}
				return protocol.Command{Kind: "set_session_roles", Data: &protocol.CommandSetSessionRolesPayload{Session: inv.Session.ID, ExpectedRevision: expected, Roles: roles}}, nil
			},
			project: applied(false),
		},
		{
			name:        "list_teams",
			description: "List teams and exact revisions in this execution Node's catalog. A team selects catalog agents by ID and may add lead instructions and a lead model. The virtual Default team (key default) is not listed and always selects the whole agent catalog. No provider configuration or credentials are returned.",
			parameters:  empty(), readOnly: true, prepare: list("list_teams"), project: listedTeams(true),
		},
		{
			name:        "add_team",
			description: "Add a team to this execution Node's catalog. Use a unique lowercase key other than default, a display name and member agent IDs from list_agents. Optional instructions extend the lead's instructions; optional model identifies an existing provider/model, never credentials. This does not change this conversation's team; use apply_team only when explicitly requested.",
			parameters:  object([]any{"key", "name"}, team(false)),
			prepare: func(_ context.Context, arguments json.RawMessage) (protocol.Command, error) {
				input, valid := catalogInput(arguments, teamFields, "key", "name")
				if !valid {
					return protocol.Command{}, invalid("Invalid team definition")
				}
				base := map[string]any{"id": protocol.NewTeamID(), "revision": 0, "key": "", "name": "", "appearance": nil,
					"description": "", "model": nil, "instructions": "", "members": []string{}}
				var team protocol.TeamProfile
				if err := catalogMerge(base, input, teamFields, &team); err != nil {
					return protocol.Command{}, err
				}
				if err := store.ValidateTeam(team); err != nil {
					return protocol.Command{}, err
				}
				return protocol.Command{Kind: "put_team", Data: &protocol.CommandPutTeamPayload{Team: team}}, nil
			},
			project: savedTeam,
		},
		{
			name:        "edit_team",
			description: "Edit one catalog team using its exact ID and decimal-string revision from list_teams. Omitted fields are preserved; explicit null clears optional fields; members replaces the complete member list. Inspect conflicts instead of silently rebasing. Existing conversations, queued turns and children keep their captures until explicit application for future admission.",
			parameters:  object(identity, team(true)),
			prepare: func(ctx context.Context, arguments json.RawMessage) (protocol.Command, error) {
				output, err := node.agentCommand(ctx, inv, protocol.Command{Kind: "list_teams"}, catalogLost)
				if err != nil {
					return protocol.Command{}, err
				}
				input, expected, err := edit(arguments, teamFields, "Inspect the team and use its exact revision")
				if err != nil {
					return protocol.Command{}, err
				}
				var id protocol.TeamID
				if err := catalogIdentity(input, &id); err != nil {
					return protocol.Command{}, err
				}
				list, listed := dispatchPayload[[]protocol.TeamProfile](output.Data)
				if !listed {
					return protocol.Command{}, connectionFault(protocol.ErrorCodeInternal, "team catalog is unavailable")
				}
				index := slices.IndexFunc(list, func(team protocol.TeamProfile) bool { return team.ID == id })
				if index < 0 {
					return protocol.Command{}, connectionFault(protocol.ErrorCodeNotFound, "Team not found")
				}
				if list[index].Revision != expected {
					return protocol.Command{}, changed
				}
				var team protocol.TeamProfile
				if err := catalogMerge(list[index], input, teamFields, &team); err != nil {
					return protocol.Command{}, err
				}
				if err := store.ValidateTeam(team); err != nil {
					return protocol.Command{}, err
				}
				return protocol.Command{Kind: "put_team", Data: &protocol.CommandPutTeamPayload{Team: team, ExpectedRevision: expected}}, nil
			},
			project: savedTeam,
		},
		{
			name:        "delete_team",
			description: "Remove exactly one team from this execution Node's catalog using its ID and decimal-string revision from list_teams. Confirm the user's intent before deletion. Member agents stay in the catalog. Existing conversations, queued turns and children retain their immutable captures; this does not delete any conversation or automatically change its team.",
			parameters:  object(identity, map[string]any{"id": text, "expected_revision": revision}),
			prepare: func(_ context.Context, arguments json.RawMessage) (protocol.Command, error) {
				input, expected, err := removal(arguments, "Inspect the team and use its exact revision")
				if err != nil {
					return protocol.Command{}, err
				}
				var id protocol.TeamID
				if err := catalogIdentity(input, &id); err != nil {
					return protocol.Command{}, err
				}
				return protocol.Command{Kind: "remove_team", Data: &protocol.CommandRemoveTeamPayload{Team: id, ExpectedRevision: expected}}, nil
			},
			project: listedTeams(false),
		},
		{
			name:        "apply_team",
			description: "Explicitly select this conversation's team for future turn admission only. Supply a team ID with its exact decimal-string revision from list_teams, or null for the Default team, which selects the whole agent catalog. Also supply this conversation's exact expected_session_revision from initialization or a completed apply receipt. Never silently rebase conflicts. This running turn, already queued turns and existing children keep their immutable captures; this cannot target another conversation.",
			parameters:  object([]any{"team", "expected_session_revision"}, map[string]any{"team": selection, "expected_session_revision": revision}),
			prepare: func(_ context.Context, arguments json.RawMessage) (protocol.Command, error) {
				rejected := invalid("Use a listed team and exact revisions")
				input, valid := catalogInput(arguments, []string{"team", "expected_session_revision"}, "team", "expected_session_revision")
				expected, exact := catalogRevision(input["expected_session_revision"])
				if !valid || !exact {
					return protocol.Command{}, rejected
				}
				var selected *protocol.TeamSelection
				if string(input["team"]) != "null" {
					fields, valid := catalogInput(input["team"], []string{"id", "revision"}, "id", "revision")
					revision, exact := catalogRevision(fields["revision"])
					var id protocol.TeamID
					if !valid || !exact || catalogIdentity(fields, &id) != nil {
						return protocol.Command{}, rejected
					}
					selected = &protocol.TeamSelection{ID: id, Revision: &revision}
				}
				return protocol.Command{Kind: "set_session_team", Data: &protocol.CommandSetSessionExecutionPayload{Session: inv.Session.ID, ExpectedRevision: expected, Execution: protocol.ConversationExecutionTeam, Team: selected}}, nil
			},
			project: applied(true),
		},
	}
	for index := range declared {
		declared[index].lost = catalogLost
	}
	return declared
}
