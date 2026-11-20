package store

import (
	"context"
	"database/sql"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Roles and teams are matched by ID and then by key: each profile creates its
// built-in roles with IDs of its own, and a key is unique per user. A role
// matched by key keeps the target's ID, and team members follow it.

func exportRoles(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	roles, err := ReadRoles(ctx, db)
	if err != nil {
		return err
	}
	for index := range roles {
		roles[index].Revision, roles[index].Source = 0, nil
	}
	slices.SortFunc(roles, func(a, b protocol.RoleProfile) int { return strings.Compare(a.ID.String(), b.ID.String()) })
	document.Roles = roles
	return nil
}

func importRoles(importer *configurationImporter, document ConfigurationDocument) error {
	ctx, db := importer.ctx, importer.db
	importer.roles = map[protocol.RoleID]protocol.RoleID{}
	for _, role := range document.Roles {
		if err := importer.entry("roles", role.Name, func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
			current, err := matchedConfiguration(ctx, db, "roles", role.ID.String(), role.Key, func(value protocol.RoleProfile) string { return value.Key }, ReadRoles)
			if err != nil {
				return "", nil, nil, err
			}
			expected := uint64(0)
			if current != nil {
				expected = current.Revision
				importer.roles[role.ID], role.ID = current.ID, current.ID
			}
			role.Source, role.Revision = nil, 0
			_, events, err := ExecuteTeamCommand(ctx, db, protocol.Command{Kind: "put_role", Data: &protocol.CommandPutRolePayload{Role: role, ExpectedRevision: expected}})
			return changedAction(current != nil), nil, events, err
		}); err != nil {
			return err
		}
	}
	return nil
}

func exportTeams(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	teams, err := ReadTeams(ctx, db)
	if err != nil {
		return err
	}
	for index := range teams {
		teams[index].Revision, teams[index].Source = 0, nil
	}
	slices.SortFunc(teams, func(a, b protocol.TeamProfile) int { return strings.Compare(a.ID.String(), b.ID.String()) })
	document.Teams = teams
	return nil
}

func importTeams(importer *configurationImporter, document ConfigurationDocument) error {
	ctx, db := importer.ctx, importer.db
	for _, team := range document.Teams {
		if err := importer.entry("teams", team.Name, func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
			current, err := matchedConfiguration(ctx, db, "teams", team.ID.String(), team.Key, func(value protocol.TeamProfile) string { return value.Key }, ReadTeams)
			if err != nil {
				return "", nil, nil, err
			}
			expected := uint64(0)
			if current != nil {
				expected, team.ID = current.Revision, current.ID
			}
			team.Members = slices.Clone(team.Members)
			for index, member := range team.Members {
				if local, matched := importer.roles[member]; matched {
					team.Members[index] = local
				}
			}
			team.Source, team.Revision = nil, 0
			_, events, err := ExecuteTeamCommand(ctx, db, protocol.Command{Kind: "put_team", Data: &protocol.CommandPutTeamPayload{Team: team, ExpectedRevision: expected}})
			return changedAction(current != nil), nil, events, err
		}); err != nil {
			return err
		}
	}
	return nil
}

// matchedConfiguration returns the current user's entry with this ID, or else
// with this key. An ID that another user owns is refused.
func matchedConfiguration[T any](ctx context.Context, db *sql.Conn, table, id, key string, keyOf func(T) string, list func(context.Context, *sql.Conn) ([]T, error)) (*T, error) {
	if err := checkConfigurationOwner(ctx, db, table, id); err != nil {
		return nil, err
	}
	current, err := readUserConfiguration[T](ctx, db, table, id)
	if err != nil || current != nil {
		return current, err
	}
	values, err := list(ctx, db)
	if err != nil {
		return nil, err
	}
	for index := range values {
		if keyOf(values[index]) == key {
			return &values[index], nil
		}
	}
	return nil, nil
}
