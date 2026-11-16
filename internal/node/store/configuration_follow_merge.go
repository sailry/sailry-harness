package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// A user merge must not discard an existing source-to-local identity binding.
// Run before promoting rows or memberships, including when a caller retains the
// surrounding transaction after reporting a merge conflict.
func checkConfigurationFollowMerge(ctx context.Context, db *sql.Conn, source, target protocol.UserID) error {
	var conflict bool
	err := db.QueryRowContext(ctx, `SELECT EXISTS(
SELECT 1 FROM configuration_follow_records s JOIN configuration_follow_records t
ON t.user=?2 AND t.category=s.category AND t.id=s.id
WHERE s.user=?1 AND (s.source!=t.source OR
CASE WHEN s.local_id='' THEN s.id ELSE s.local_id END != CASE WHEN t.local_id='' THEN t.id ELSE t.local_id END))`, source.String(), target.String()).Scan(&conflict)
	if err != nil {
		return err
	}
	if conflict {
		return fault(protocol.ErrorCodeConflict, "users have configuration copies owned by different controllers")
	}
	return nil
}

// MergeConfigurationFollowUsers runs after live configuration and plugin rows
// move. The surviving user's choices win; metadata follows only a copied row
// actually owned by that user. Archived collisions retain their original owner.
func MergeConfigurationFollowUsers(ctx context.Context, db *sql.Conn, source, target protocol.User) error {
	if source.ID == target.ID {
		return nil
	}
	from, to := source.ID.String(), target.ID.String()
	if _, err := db.ExecContext(ctx, `INSERT INTO configuration_follow_sources(user,source,revision) SELECT ?2,source,revision FROM configuration_follow_sources WHERE user=?1 ON CONFLICT(user) DO NOTHING`, from, to); err != nil {
		return err
	}
	ctx = WithUser(ctx, target)
	singletonMatches := map[string]bool{}
	var raw []byte
	var previousRevision uint64
	err := db.QueryRowContext(ctx, "SELECT revision,config FROM user_defaults WHERE user=?1", from).Scan(&previousRevision, &raw)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if err == nil {
		var previous protocol.SessionConfig
		if err = json.Unmarshal(raw, &previous); err != nil {
			return err
		}
		current, err := ReadDefaults(ctx, db)
		if err != nil {
			return err
		}
		singletonMatches["defaults"] = current.Revision == previousRevision && reflect.DeepEqual(current.Config, &previous)
	}
	previousTeam, err := readBody[protocol.TeamSettings](ctx, db, "SELECT body FROM user_team_settings WHERE user=?1", from)
	if err != nil {
		return err
	}
	if previousTeam != nil {
		current, err := ReadTeamSettings(ctx, db)
		if err != nil {
			return err
		}
		singletonMatches["team_settings"] = reflect.DeepEqual(current, *previousTeam)
	}
	for _, category := range ConfigurationCategories {
		owner := "0"
		switch category {
		case "providers":
			owner = `EXISTS(SELECT 1 FROM providers p WHERE p.id=CASE WHEN r.local_id='' THEN r.id ELSE r.local_id END AND p.user=?2 AND json_type(p.body,'$.provider.source')='object')`
		case "roles", "teams", "ssh_profiles", "database_profiles":
			owner = `EXISTS(SELECT 1 FROM ` + category + ` p WHERE p.id=CASE WHEN r.local_id='' THEN r.id ELSE r.local_id END AND p.user=?2 AND json_type(p.body,'$.source')='object')`
		case "plugins", "plugin_settings":
			owner = `EXISTS(SELECT 1 FROM plugin_user_installations p WHERE p.name=r.id AND p.user=?2 AND json_type(p.body,'$.summary.source')='object' AND p.body=(SELECT body FROM plugin_user_installations WHERE user=?1 AND name=r.id))`
		case "connections":
			owner = `EXISTS(SELECT 1 FROM plugin_connections p WHERE p.id=CASE WHEN r.local_id='' THEN r.id ELSE r.local_id END AND p.user=?2)`
		case "defaults", "team_settings":
			if singletonMatches[category] {
				owner = "1"
			}

		}
		query := `INSERT INTO configuration_follow_records(user,category,id,source,revision,digest,state,materialized,message,body,local_id)
SELECT ?2,r.category,r.id,r.source,r.revision,r.digest,r.state,r.materialized,r.message,r.body,CASE WHEN r.local_id='' AND r.materialized=1 THEN r.id ELSE r.local_id END FROM configuration_follow_records r
WHERE r.user=?1 AND r.category=?3 AND ((r.materialized=1 AND (` + owner + `)) OR (r.materialized=0 AND EXISTS(SELECT 1 FROM configuration_follow_sources s WHERE s.user=?2 AND s.source=r.source)))
ON CONFLICT(user,category,id) DO NOTHING`
		if _, err := db.ExecContext(ctx, query, from, to, category); err != nil {
			return err
		}
	}
	for _, query := range []string{
		`INSERT INTO configuration_follow_versions(user,category,source,revision,digest) SELECT ?2,v.category,v.source,v.revision,v.digest FROM configuration_follow_versions v WHERE v.user=?1 AND EXISTS(SELECT 1 FROM configuration_follow_sources s WHERE s.user=?2 AND s.source=v.source) ON CONFLICT(user,category,source) DO NOTHING`,
		`INSERT INTO configuration_follow_mcp_overrides(user,plugin,server) SELECT ?2,o.plugin,o.server FROM configuration_follow_mcp_overrides o WHERE o.user=?1 AND EXISTS(SELECT 1 FROM configuration_follow_records r WHERE r.user=?2 AND r.category='plugins' AND r.id=o.plugin AND r.materialized=1) ON CONFLICT(user,plugin,server) DO NOTHING`,
	} {
		if _, err := db.ExecContext(ctx, query, from, to); err != nil {
			return err
		}
	}
	if source.Local && target.Local {
		for _, query := range []string{
			`INSERT INTO configuration_follow_targets(user,host,revision,configuration,secrets,failure) SELECT ?2,host,revision,configuration,secrets,failure FROM configuration_follow_targets WHERE user=?1 ON CONFLICT(user,host) DO NOTHING`,
			`INSERT INTO configuration_follow_deliveries(user,host,category,request) SELECT ?2,host,category,request FROM configuration_follow_deliveries WHERE user=?1 ON CONFLICT(user,host,category) DO NOTHING`,
		} {
			if _, err := db.ExecContext(ctx, query, from, to); err != nil {
				return err
			}
		}
	}
	return nil
}
