package store

import (
	"context"
	"database/sql"
	"slices"
	"sort"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CaptureSessionRoles(ctx context.Context, db *sql.Conn, node protocol.NodeID, execution protocol.ConversationExecution, team *protocol.TeamSelection, models map[protocol.RoleID]protocol.RoleModel) (protocol.RoleSnapshot, error) {
	value := protocol.DefaultRoleSnapshot()
	value.Execution = execution
	value.Models = models
	if execution == protocol.ConversationExecutionModel {
		if team != nil || len(models) > 0 {
			return value, invalid("model execution cannot select a delegation roster")
		}
		return value, nil
	}
	if execution != protocol.ConversationExecutionTeam {
		return value, invalid("invalid execution selection")
	}
	var err error
	if team == nil {
		value.Profiles, err = ReadRoles(ctx, db)
	} else {
		selected, e := readBody[protocol.TeamProfile](ctx, db, "SELECT body FROM teams WHERE id=?1", team.ID.String())
		if e != nil {
			return value, e
		}
		if selected == nil {
			return value, fault(protocol.ErrorCodeNotFound, "team does not exist")
		}
		if team.Revision != nil {
			if e = checkRevision(selected.Revision, *team.Revision); e != nil {
				return value, e
			}
		}
		value.Team = &protocol.TeamReference{ID: selected.ID, Revision: selected.Revision}
		value.Instructions = selected.Instructions
		for _, id := range selected.Members {
			role, e := readBody[protocol.RoleProfile](ctx, db, "SELECT body FROM roles WHERE id=?1", id.String())
			if e != nil {
				return value, e
			}
			if role == nil {
				return value, fault(protocol.ErrorCodeNotFound, "team role does not exist")
			}
			value.Profiles = append(value.Profiles, *role)
		}
	}
	if err != nil {
		return value, err
	}
	return completeRoleSnapshot(ctx, db, node, value)
}
func completeRoleSnapshot(ctx context.Context, db *sql.Conn, node protocol.NodeID, value protocol.RoleSnapshot) (protocol.RoleSnapshot, error) {
	required := map[protocol.ProviderID]bool{}
	ids := map[protocol.RoleID]bool{}
	keys := map[string]bool{}
	for _, role := range value.Profiles {
		if err := validateRole(role); err != nil {
			return value, err
		}
		if ids[role.ID] || keys[role.Key] {
			return value, invalid("session roles must have distinct identities and keys")
		}
		ids[role.ID], keys[role.Key] = true, true
		if role.Model != nil {
			required[role.Model.Provider] = true
		}
	}
	for id, model := range value.Models {
		if !ids[id] {
			return value, invalid("member model requires a role in the session roster")
		}
		required[model.Provider] = true
	}
	captured := value.Providers
	value.Providers = []protocol.ConversationProvider{}
	ordered := []protocol.ProviderID{}
	for id := range required {
		ordered = append(ordered, id)
	}
	sort.Slice(ordered, func(i, j int) bool { return ordered[i].String() < ordered[j].String() })
	for _, id := range ordered {
		provider, err := listedRoleProvider(ctx, db, id)
		if err != nil {
			index := slices.IndexFunc(captured, func(p protocol.ConversationProvider) bool { return p.ID == id })
			if index < 0 {
				return value, err
			}
			provider = captured[index]
		}
		provider, err = CaptureProvider(ctx, db, provider)
		if err != nil {
			return value, err
		}
		if provider.Credential != nil && provider.Credential.Node != node {
			return value, fault(protocol.ErrorCodePermissionDenied, "role provider belongs to another Node")
		}
		for _, role := range value.Profiles {
			if role.Model != nil && role.Model.Provider == id {
				if err = validateRoleModel(provider, *role.Model); err != nil {
					return value, err
				}
			}
		}
		for _, model := range value.Models {
			if model.Provider == id {
				if err = validateRoleModel(provider, model); err != nil {
					return value, err
				}
			}
		}
		value.Providers = append(value.Providers, provider)
	}
	return value, nil
}
func SelectSessionRoles(ctx context.Context, db *sql.Conn, node protocol.NodeID, previous protocol.RoleSnapshot, references []protocol.RoleReference) (protocol.RoleSnapshot, error) {
	previous.Execution = protocol.ConversationExecutionTeam
	previous.Profiles = []protocol.RoleProfile{}
	ids := map[protocol.RoleID]bool{}
	for _, reference := range references {
		if ids[reference.ID] {
			return previous, invalid("session roles must be distinct")
		}
		ids[reference.ID] = true
		role, err := readBody[protocol.RoleProfile](ctx, db, "SELECT body FROM roles WHERE id=?1", reference.ID.String())
		if err != nil {
			return previous, err
		}
		if role == nil {
			return previous, fault(protocol.ErrorCodeNotFound, "role does not exist")
		}
		if err = checkRevision(role.Revision, reference.Revision); err != nil {
			return previous, err
		}
		previous.Profiles = append(previous.Profiles, *role)
	}
	models := map[protocol.RoleID]protocol.RoleModel{}
	for id, model := range previous.Models {
		if ids[id] {
			models[id] = model
		}
	}
	previous.Models = models
	return completeRoleSnapshot(ctx, db, node, previous)
}
func AssignSessionMemberModel(ctx context.Context, db *sql.Conn, node protocol.NodeID, previous protocol.RoleSnapshot, role protocol.RoleID, model *protocol.RoleModel) (protocol.RoleSnapshot, error) {
	if !slices.ContainsFunc(previous.Profiles, func(p protocol.RoleProfile) bool { return p.ID == role }) {
		return previous, fault(protocol.ErrorCodeNotFound, "role is not in the session roster")
	}
	models := map[protocol.RoleID]protocol.RoleModel{}
	for id, v := range previous.Models {
		models[id] = v
	}
	if model == nil {
		delete(models, role)
	} else {
		models[role] = *model
	}
	previous.Models = models
	return completeRoleSnapshot(ctx, db, node, previous)
}
func RefreshTurnRoles(ctx context.Context, db *sql.Conn, snapshot *protocol.RoleSnapshot) error {
	for index, provider := range snapshot.Providers {
		captured, err := CaptureProvider(ctx, db, provider)
		if err != nil {
			return err
		}
		snapshot.Providers[index] = captured
	}
	return nil
}
func roleModelConfig(base protocol.SessionConfig, provider protocol.ConversationProvider, selection protocol.RoleModel) (protocol.SessionConfig, error) {
	if err := validateRoleModel(provider, selection); err != nil {
		return base, err
	}
	base.Provider, base.Model, base.Credential = provider.ID, selection.Model, provider.Credential
	for _, model := range provider.Models {
		if model.ID == selection.Model {
			base.Effort = model.DefaultEffort
			break
		}
	}
	if selection.Effort != nil {
		base.Effort = *selection.Effort
	}
	features := base.Features
	if selection.Features != nil {
		features = *selection.Features
	}
	base.Features = map[string]string{}
	for key, value := range features {
		candidate := base
		candidate.Features = map[string]string{key: value}
		if providers.ValidateSelection(provider, candidate) == nil {
			base.Features[key] = value
		}
	}
	return base, nil
}
func sessionTeamCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, command protocol.Command) (protocol.Output, []protocol.Event, error) {
	var id protocol.SessionID
	var revision uint64
	switch v := command.Data.(type) {
	case *protocol.CommandSetSessionRolesPayload:
		id, revision = v.Session, v.ExpectedRevision
	case *protocol.CommandSetSessionExecutionPayload:
		id, revision = v.Session, v.ExpectedRevision
	case *protocol.CommandSetSessionMemberModelPayload:
		id, revision = v.Session, v.ExpectedRevision
	default:
		return protocol.Output{}, nil, invalid("invalid session team command")
	}
	session, err := checkedSession(ctx, db, id, revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	switch v := command.Data.(type) {
	case *protocol.CommandSetSessionRolesPayload:
		session.Roles, err = SelectSessionRoles(ctx, db, node, session.Roles, v.Roles)
	case *protocol.CommandSetSessionMemberModelPayload:
		session.Roles, err = AssignSessionMemberModel(ctx, db, node, session.Roles, v.Role, v.Model)
	case *protocol.CommandSetSessionExecutionPayload:
		session.Roles, err = CaptureSessionRoles(ctx, db, node, v.Execution, v.Team, v.Models)
		if err == nil && v.Execution == protocol.ConversationExecutionTeam {
			session.Config, err = configureSessionTeam(ctx, db, session.Config, v.Team)
		}
	}
	if err != nil {
		return protocol.Output{}, nil, err
	}
	return reviseSession(ctx, db, session)
}

func configureSessionTeam(ctx context.Context, db *sql.Conn, config protocol.SessionConfig, selection *protocol.TeamSelection) (protocol.SessionConfig, error) {
	if selection == nil {
		return config, nil
	}
	team, err := readBody[protocol.TeamProfile](ctx, db, "SELECT body FROM teams WHERE id=?1", selection.ID.String())
	if err != nil {
		return config, err
	}
	if team.Model == nil {
		return config, nil
	}
	provider, err := listedRoleProvider(ctx, db, team.Model.Provider)
	if err != nil {
		return config, err
	}
	return roleModelConfig(config, provider, *team.Model)
}
