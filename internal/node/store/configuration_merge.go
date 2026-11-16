package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// MergeConfigurationUsers promotes nonconflicting personal configuration in the
// identity merge transaction. The caller supplies the target's final local flag;
// canonical conflicts stay archived under the merged source user.
func MergeConfigurationUsers(ctx context.Context, db *sql.Conn, source, target protocol.User) error {
	if source.ID == target.ID {
		return nil
	}
	roles, err := readBodies[protocol.RoleProfile](ctx, db, "SELECT body FROM roles WHERE COALESCE(user,(SELECT id FROM users WHERE local=1))=?1 ORDER BY key", source.ID.String())
	if err != nil {
		return err
	}
	targetRoles, err := readBodies[protocol.RoleProfile](ctx, db, "SELECT body FROM roles WHERE user=?1 ORDER BY key", target.ID.String())
	if err != nil {
		return err
	}
	byKey := map[string]protocol.RoleID{}
	for _, role := range targetRoles {
		byKey[role.Key] = role.ID
	}
	remap := map[protocol.RoleID]protocol.RoleID{}
	// Move every source key aside before the surviving user gains local keys.
	for _, table := range []string{"roles", "teams"} {
		if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET key=?1||':'||json_extract(body,'$.key') WHERE COALESCE(user,(SELECT id FROM users WHERE local=1))=?1", source.ID.String()); err != nil {
			return err
		}
	}
	for _, role := range roles {
		if existing, ok := byKey[role.Key]; ok {
			remap[role.ID] = existing
			continue
		}
		if _, err := db.ExecContext(ctx, "UPDATE roles SET user=?2,key=?3 WHERE id=?1", role.ID.String(), target.ID.String(), configurationKey(target, role.Key)); err != nil {
			return err
		}
	}
	teams, err := readBodies[protocol.TeamProfile](ctx, db, "SELECT body FROM teams WHERE COALESCE(user,(SELECT id FROM users WHERE local=1))=?1 ORDER BY key", source.ID.String())
	if err != nil {
		return err
	}
	for _, team := range teams {
		var conflict bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM teams WHERE user=?1 AND json_extract(body,'$.key')=?2)", target.ID.String(), team.Key).Scan(&conflict); err != nil {
			return err
		}
		if conflict {
			continue
		}
		members := []protocol.RoleID{}
		seen := map[protocol.RoleID]bool{}
		for _, id := range team.Members {
			if mapped, ok := remap[id]; ok {
				id = mapped
			}
			if !seen[id] {
				members = append(members, id)
				seen[id] = true
			}
		}
		team.Members = members
		body, err := encode(team)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE teams SET user=?2,key=?3,body=?4 WHERE id=?1", team.ID.String(), target.ID.String(), configurationKey(target, team.Key), body); err != nil {
			return err
		}
	}
	for _, table := range []string{"providers", "ssh_profiles", "database_profiles"} {
		if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET user=?2 WHERE COALESCE(user,(SELECT id FROM users WHERE local=1))=?1", source.ID.String(), target.ID.String()); err != nil {
			return err
		}
	}
	// Provider owner is public metadata; SQL ownership stays authoritative.
	if _, err := db.ExecContext(ctx, "UPDATE providers SET body=json_set(body,'$.provider.owner',?1) WHERE user=?1 AND json_type(body,'$.provider')='object'", target.ID.String()); err != nil {
		return err
	}
	for _, table := range []string{"roles", "teams"} {
		if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET key=CASE WHEN ?2 THEN json_extract(body,'$.key') ELSE ?1||':'||json_extract(body,'$.key') END WHERE user=?1", target.ID.String(), target.Local); err != nil {
			return err
		}
	}
	if err := mergeUserSingleton(ctx, db, "defaults", "config", source, target); err != nil {
		return err
	}
	return mergeUserSingleton(ctx, db, "team_settings", "body", source, target)
}

func mergeUserSingleton(ctx context.Context, db *sql.Conn, table, column string, source, target protocol.User) error {
	read := func(id protocol.UserID) ([]byte, error) {
		var local bool
		if err := db.QueryRowContext(ctx, "SELECT local FROM users WHERE id=?1", id.String()).Scan(&local); err != nil {
			return nil, err
		}
		var raw []byte
		var err error
		query := "SELECT " + column + " FROM user_" + table + " WHERE user=?1"
		if table == "defaults" {
			query = "SELECT json_object('revision',revision,'config',json(config)) FROM user_defaults WHERE user=?1"
		}
		if local {
			query = "SELECT " + column + " FROM " + table + " WHERE singleton=1"
			if table == "defaults" {
				query = "SELECT json_object('revision',revision,'config',json(config)) FROM defaults WHERE singleton=1"
			}
			err = db.QueryRowContext(ctx, query).Scan(&raw)
		} else {
			err = db.QueryRowContext(ctx, query, id.String()).Scan(&raw)
		}
		if errors.Is(err, sql.ErrNoRows) {
			return nil, nil
		}
		return raw, err
	}
	before, err := read(source.ID)
	if err != nil {
		return err
	}
	chosen, err := read(target.ID)
	if err != nil {
		return err
	}
	if len(chosen) == 0 {
		chosen = before
	}
	write := func(id protocol.UserID, local bool, raw []byte) error {
		if len(raw) == 0 {
			return nil
		}
		if table == "defaults" {
			var value protocol.Defaults
			if err := json.Unmarshal(raw, &value); err != nil {
				return err
			}
			body, err := encode(value.Config)
			if err != nil {
				return err
			}
			if local {
				_, err = db.ExecContext(ctx, "INSERT INTO defaults(singleton,revision,config) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision,config=excluded.config", value.Revision, body)
			} else {
				_, err = db.ExecContext(ctx, "INSERT INTO user_defaults(user,revision,config) VALUES(?1,?2,?3) ON CONFLICT(user) DO UPDATE SET revision=excluded.revision,config=excluded.config", id.String(), value.Revision, body)
			}
			return err
		}
		if local {
			_, err := db.ExecContext(ctx, "INSERT INTO team_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", raw)
			return err
		}
		_, err := db.ExecContext(ctx, "INSERT INTO user_team_settings(user,body) VALUES(?1,?2) ON CONFLICT(user) DO UPDATE SET body=excluded.body", id.String(), raw)
		return err
	}
	if err := write(source.ID, false, before); err != nil {
		return err
	}
	return write(target.ID, target.Local, chosen)
}
