package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadTeamSettings(ctx context.Context, db *sql.Conn) (protocol.TeamSettings, error) {
	stored, err := readBody[protocol.TeamSettings](ctx, db, "SELECT body FROM team_settings WHERE singleton=1")
	if err != nil {
		return protocol.TeamSettings{}, err
	}
	if stored == nil {
		return protocol.DefaultTeamSettings(), nil
	}
	return *stored, nil
}
func SupportsTeamRead(kind string) bool {
	return kind == "list_roles" || kind == "list_teams" || kind == "read_team_settings"
}
func SupportsTeamCommand(kind string) bool {
	return kind == "put_role" || kind == "remove_role" || kind == "put_team" || kind == "remove_team" || kind == "save_team_settings"
}
func ReadTeamCommand(ctx context.Context, db *sql.Conn, command protocol.Command) (protocol.Output, error) {
	switch command.Kind {
	case "list_roles":
		value, err := ReadRoles(ctx, db)
		return protocol.Output{Kind: "roles", Data: value}, err
	case "list_teams":
		value, err := ReadTeams(ctx, db)
		return protocol.Output{Kind: "teams", Data: value}, err
	case "read_team_settings":
		value, err := ReadTeamSettings(ctx, db)
		return protocol.Output{Kind: "team_settings", Data: value}, err
	}
	return protocol.Output{}, invalid("unsupported team read")
}
func ExecuteTeamCommand(ctx context.Context, db *sql.Conn, command protocol.Command) (protocol.Output, []protocol.Event, error) {
	switch v := command.Data.(type) {
	case *protocol.CommandPutRolePayload:
		value, err := putRole(ctx, db, v.Role, v.ExpectedRevision)
		return protocol.Output{Kind: "role", Data: value}, []protocol.Event{{Kind: "role_changed", Data: value}}, err
	case *protocol.CommandPutTeamPayload:
		value, err := putTeam(ctx, db, v.Team, v.ExpectedRevision)
		return protocol.Output{Kind: "team", Data: value}, []protocol.Event{{Kind: "team_changed", Data: value}}, err
	case *protocol.CommandRemoveRolePayload:
		current, err := readBody[protocol.RoleProfile](ctx, db, "SELECT body FROM roles WHERE id=?1", v.Role.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if current == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "role does not exist")
		}
		if err = checkRevision(current.Revision, v.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err = db.ExecContext(ctx, "DELETE FROM roles WHERE id=?1", v.Role.String()); err != nil {
			return protocol.Output{}, nil, err
		}
		teams, err := ReadTeams(ctx, db)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		changed := []protocol.TeamProfile{}
		for _, team := range teams {
			members := team.Members[:0]
			for _, id := range team.Members {
				if id != v.Role {
					members = append(members, id)
				}
			}
			if len(members) == len(team.Members) {
				continue
			}
			team.Members = members
			team.Revision, err = nextRevision(team.Revision)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			if err = writeTeam(ctx, db, team); err != nil {
				return protocol.Output{}, nil, err
			}
			changed = append(changed, team)
		}
		remaining, err := ReadRoles(ctx, db)
		return protocol.Output{Kind: "roles", Data: remaining}, []protocol.Event{{Kind: "role_removed", Data: protocol.EventRoleRemovedPayload{ID: v.Role, Teams: changed}}}, err
	case *protocol.CommandRemoveTeamPayload:
		current, err := readBody[protocol.TeamProfile](ctx, db, "SELECT body FROM teams WHERE id=?1", v.Team.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if current == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "team does not exist")
		}
		if err = checkRevision(current.Revision, v.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM teams WHERE id=?1", v.Team.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		remaining, err := ReadTeams(ctx, db)
		return protocol.Output{Kind: "teams", Data: remaining}, []protocol.Event{{Kind: "team_removed", Data: protocol.EventTeamRemovedPayload{ID: v.Team}}}, err
	case *protocol.TeamSettings:
		if v.ChildConcurrency == 0 {
			return protocol.Output{}, nil, invalid("child concurrency must be a positive integer")
		}
		current, err := ReadTeamSettings(ctx, db)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if err = checkRevision(current.Revision, v.Revision); err != nil {
			return protocol.Output{}, nil, err
		}
		value := *v
		value.Revision, err = nextRevision(current.Revision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		body, err := encode(value)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO team_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
		return protocol.Output{Kind: "team_settings", Data: value}, []protocol.Event{{Kind: "team_settings_changed", Data: protocol.EventTeamSettingsChangedPayload{Revision: value.Revision}}}, err
	}
	return protocol.Output{}, nil, invalid("unsupported team command")
}
