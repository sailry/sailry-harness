package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadRoles(ctx context.Context, db *sql.Conn) ([]protocol.RoleProfile, error) {
	return readBodies[protocol.RoleProfile](ctx, db, "SELECT body FROM roles ORDER BY key")
}
func ReadTeams(ctx context.Context, db *sql.Conn) ([]protocol.TeamProfile, error) {
	return readBodies[protocol.TeamProfile](ctx, db, "SELECT body FROM teams ORDER BY key")
}

func roleKey(key string) bool {
	if key == "" || key[0] < 'a' || key[0] > 'z' {
		return false
	}
	for _, c := range []byte(key) {
		if (c < 'a' || c > 'z') && (c < '0' || c > '9') && c != '_' && c != '-' {
			return false
		}
	}
	last := key[len(key)-1]
	return last >= 'a' && last <= 'z' || last >= '0' && last <= '9'
}
func validRoleToolName(value string) bool {
	return strings.TrimSpace(value) != "" && len(value) <= 256 && !strings.ContainsFunc(value, unicode.IsControl)
}
func validRolePackage(value string) bool {
	if value == "" || len(value) > 64 || strings.Contains(value, "--") || strings.Contains(value, "..") {
		return false
	}
	alnum := func(c byte) bool { return c >= 'a' && c <= 'z' || c >= '0' && c <= '9' }
	if !alnum(value[0]) || !alnum(value[len(value)-1]) {
		return false
	}
	for _, c := range []byte(value) {
		if !alnum(c) && c != '-' && c != '.' {
			return false
		}
	}
	return true
}

// ValidateRole reports what a role definition alone makes invalid. A caller
// that asks for approval of a definition checks it first.
func ValidateRole(role protocol.RoleProfile) error {
	if role.ID == (protocol.RoleID{}) || !roleKey(role.Key) || strings.TrimSpace(role.Name) == "" || role.MaxTurns != nil && *role.MaxTurns == 0 {
		return invalid("role key, name or turn limit is invalid")
	}
	if role.Model != nil && strings.TrimSpace(role.Model.Model) == "" {
		return invalid("role model identifier is invalid")
	}
	if role.Appearance != nil {
		// A role's icon is a project icon or a generated avatar's seed
		// (protocol.RoleAvatarSeed), which the Desktop draws.
		appearance := *role.Appearance
		if _, ok := protocol.RoleAvatarSeed(appearance.Icon); ok {
			appearance.Icon = protocol.DefaultProjectsAppearance().Icon
		}
		if err := validateAppearance(appearance); err != nil {
			return err
		}
	}
	seen := map[string]bool{}
	for _, skill := range role.Skills {
		if strings.TrimSpace(skill) == "" || seen[skill] {
			return invalid("role skills must be distinct and nonempty")
		}
		seen[skill] = true
	}
	if role.Tools != nil {
		if len(*role.Tools) > 128 {
			return invalid("role tool selection exceeds the limit")
		}
		seen = map[string]bool{}
		for _, tool := range *role.Tools {
			body, err := json.Marshal(tool)
			if err != nil || tool.Kind == "plugin" {
				return invalid("role tools must select valid builtins or package tools")
			}
			if seen[string(body)] {
				return invalid("role tools must be distinct")
			}
			seen[string(body)] = true
			switch tool.Kind {
			case "builtin":
				v, ok := filePayload[protocol.PluginConversationToolBuiltinPayload](tool.Data)
				if !ok || !validRoleToolName(v.Name) {
					return invalid("role builtin tool is invalid")
				}
			case "package":
				v, ok := filePayload[protocol.PluginConversationToolPackagePayload](tool.Data)
				if !ok || !validRoleToolName(v.Name) || !validRolePackage(v.Package) {
					return invalid("role package tool is invalid")
				}
			default:
				return invalid("role tool kind is invalid")
			}
		}
	}
	return nil
}
func validateRoleModel(provider protocol.ConversationProvider, selection protocol.RoleModel) error {
	if !provider.Enabled {
		return fault(protocol.ErrorCodeNotConfigured, "role provider is unavailable")
	}
	index := slices.IndexFunc(provider.Models, func(model protocol.ConversationModel) bool { return model.ID == selection.Model })
	if index < 0 {
		return fault(protocol.ErrorCodeNotConfigured, "role model is unavailable")
	}
	model := provider.Models[index]
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, selection.Model, model.DefaultEffort
	if selection.Effort != nil {
		if !model.Reasoning {
			return invalid("role reasoning effort is unavailable")
		}
		config.Effort = *selection.Effort
	}
	if selection.Features != nil {
		config.Features = *selection.Features
	}
	return providers.ValidateSelection(provider, config)
}
func listedRoleProvider(ctx context.Context, db *sql.Conn, id protocol.ProviderID) (protocol.ConversationProvider, error) {
	stored, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", id.String())
	if err != nil {
		return protocol.ConversationProvider{}, err
	}
	if stored == nil || !stored.Listed || stored.Provider == nil {
		return protocol.ConversationProvider{}, fault(protocol.ErrorCodeNotConfigured, "role provider is unavailable")
	}
	return *stored.Provider, nil
}
func putRole(ctx context.Context, db *sql.Conn, role protocol.RoleProfile, expected uint64) (protocol.RoleProfile, error) {
	if err := ValidateRole(role); err != nil {
		return role, err
	}
	current, err := readBody[protocol.RoleProfile](ctx, db, "SELECT body FROM roles WHERE id=?1", role.ID.String())
	if err != nil {
		return role, err
	}
	revision := uint64(0)
	if current != nil {
		revision = current.Revision
	}
	if err = checkRevision(revision, expected); err != nil {
		return role, err
	}
	var used bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM roles WHERE key=?1 AND id<>?2)", role.Key, role.ID.String()).Scan(&used); err != nil {
		return role, err
	}
	if used {
		return role, fault(protocol.ErrorCodeConflict, "role key is already in use")
	}
	if role.Model != nil {
		provider, err := listedRoleProvider(ctx, db, role.Model.Provider)
		if err != nil {
			return role, err
		}
		if err = validateRoleModel(provider, *role.Model); err != nil {
			return role, err
		}
	}
	if role.Appearance == nil {
		role.Appearance = appearanceFor(role.Key)
	}
	role.Revision, err = nextRevision(revision)
	if err != nil {
		return role, err
	}
	body, err := encode(role)
	if err != nil {
		return role, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO roles(id,key,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET key=excluded.key,body=excluded.body", role.ID.String(), role.Key, body)
	return role, err
}

// ValidateTeam reports what a team definition alone makes invalid, before
// its members and model are resolved against the catalog.
func ValidateTeam(team protocol.TeamProfile) error {
	if team.ID == (protocol.TeamID{}) || !roleKey(team.Key) || team.Key == "default" || strings.TrimSpace(team.Name) == "" {
		return invalid("team key or name is invalid")
	}
	if team.Appearance != nil {
		if err := validateAppearance(*team.Appearance); err != nil {
			return err
		}
	}
	if team.Model != nil && strings.TrimSpace(team.Model.Model) == "" {
		return invalid("team model identifier is invalid")
	}
	seen := map[protocol.RoleID]bool{}
	for _, id := range team.Members {
		if seen[id] {
			return invalid("team members must be distinct")
		}
		seen[id] = true
	}
	return nil
}
func putTeam(ctx context.Context, db *sql.Conn, team protocol.TeamProfile, expected uint64) (protocol.TeamProfile, error) {
	if err := ValidateTeam(team); err != nil {
		return team, err
	}
	current, err := readBody[protocol.TeamProfile](ctx, db, "SELECT body FROM teams WHERE id=?1", team.ID.String())
	if err != nil {
		return team, err
	}
	revision := uint64(0)
	if current != nil {
		revision = current.Revision
	}
	if err = checkRevision(revision, expected); err != nil {
		return team, err
	}
	var used bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM teams WHERE key=?1 AND id<>?2)", team.Key, team.ID.String()).Scan(&used); err != nil {
		return team, err
	}
	if used {
		return team, fault(protocol.ErrorCodeConflict, "team key is already in use")
	}
	for _, id := range team.Members {
		role, err := readBody[protocol.RoleProfile](ctx, db, "SELECT body FROM roles WHERE id=?1", id.String())
		if err != nil {
			return team, err
		}
		if role == nil {
			return team, fault(protocol.ErrorCodeNotFound, "team role does not exist")
		}
	}
	if team.Model != nil {
		provider, err := listedRoleProvider(ctx, db, team.Model.Provider)
		if err != nil {
			return team, err
		}
		if err = validateRoleModel(provider, *team.Model); err != nil {
			return team, err
		}
	}
	if team.Appearance == nil {
		team.Appearance = appearanceFor(team.Key)
	}
	team.Revision, err = nextRevision(revision)
	if err != nil {
		return team, err
	}
	err = writeTeam(ctx, db, team)
	return team, err
}
func writeTeam(ctx context.Context, db *sql.Conn, team protocol.TeamProfile) error {
	body, err := encode(team)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO teams(id,key,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET key=excluded.key,body=excluded.body", team.ID.String(), team.Key, body)
	return err
}
