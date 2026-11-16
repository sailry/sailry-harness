package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// MergeIdentityUsers runs inside the identity command transaction. All devices
// and owned resources move together; conflicting personal records remain under
// the merged source, which public principal resolution can no longer select.
func MergeIdentityUsers(ctx context.Context, db *sql.Conn, sourceID, targetID protocol.UserID) (protocol.User, error) {
	source, err := ResolveUser(ctx, db, sourceID)
	if err != nil {
		return protocol.User{}, err
	}
	target, err := ResolveUser(ctx, db, targetID)
	if err != nil {
		return protocol.User{}, err
	}
	if source.ID == target.ID {
		return target, nil
	}
	if err := checkConfigurationFollowMerge(ctx, db, source.ID, target.ID); err != nil {
		return target, err
	}
	target.Local = target.Local || source.Local
	target.Administrator = target.Administrator || source.Administrator || target.Local
	if err := InitializeIdentity(ctx, db); err != nil {
		return target, err
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO identity_turn_origins(turn,user)
SELECT id,user FROM turns WHERE user=?1 ON CONFLICT(turn) DO NOTHING`, source.ID.String()); err != nil {
		return target, err
	}
	if err := MergeConfigurationUsers(ctx, db, source, target); err != nil {
		return target, err
	}
	if err := materializeLegacyPersonalSettings(ctx, db); err != nil {
		return target, err
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO user_personal_settings(user,kind,body) SELECT ?2,kind,body FROM user_personal_settings WHERE user=?1 ON CONFLICT(user,kind) DO NOTHING`, source.ID.String(), target.ID.String()); err != nil {
		return target, err
	}
	for _, table := range []string{"projects", "sessions", "worktrees", "terminals", "notifications"} {
		if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET owner=?2 WHERE owner=?1", source.ID.String(), target.ID.String()); err != nil {
			return target, err
		}
	}
	for _, table := range []string{"turns", "link_peers", "identity_memberships", "identity_sponsorships", "identity_deliveries"} {
		if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET user=?2 WHERE user=?1", source.ID.String(), target.ID.String()); err != nil {
			return target, err
		}
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO identity_merge_conflicts(source,target,kind,key,body) SELECT ?1,?2,'directory','snapshot',hosts FROM identity_directory WHERE user=?1 ON CONFLICT(source,kind,key) DO NOTHING", source.ID.String(), target.ID.String()); err != nil {
		return target, err
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO identity_directory(user,revision,hosts) SELECT ?2,revision,hosts FROM identity_directory WHERE user=?1 ON CONFLICT(user) DO NOTHING`, source.ID.String(), target.ID.String()); err != nil {
		return target, err
	}
	// Release the unique local flag before assigning it to the surviving user.
	if _, err := db.ExecContext(ctx, "UPDATE users SET local=0,administrator=0 WHERE id=?1", source.ID.String()); err != nil {
		return target, err
	}
	if _, err := db.ExecContext(ctx, "UPDATE users SET local=?2,administrator=?3 WHERE id=?1", target.ID.String(), target.Local, target.Administrator); err != nil {
		return target, err
	}
	if err := MergePluginUsers(ctx, db, source, target); err != nil {
		return target, err
	}
	if err := MergeConfigurationFollowUsers(ctx, db, source, target); err != nil {
		return target, err
	}
	if _, err := db.ExecContext(ctx, "UPDATE users SET state='merged',merged_into=?2 WHERE id=?1", source.ID.String(), target.ID.String()); err != nil {
		return target, err
	}
	return target, nil
}
