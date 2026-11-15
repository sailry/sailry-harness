package store

import (
	"context"
	"database/sql"
	"errors"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUsersAndDirectionalMembership(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		local, err := CallerUser(ctx, db, node, node)
		if err != nil {
			return err
		}
		if !local.Local || !local.Administrator {
			return errors.New("local user is not administrator")
		}
		host, first, second := protocol.NodeID{2}, protocol.NodeID{3}, protocol.NodeID{4}
		if err := PairLinkPeer(ctx, db, host, link.DirectionHost); err != nil {
			return err
		}
		if _, err := CallerUser(ctx, db, node, host); err == nil {
			return errors.New("outbound host gained controller membership")
		}
		if err := PairLinkPeer(ctx, db, first, link.DirectionController); err != nil {
			return err
		}
		owner, err := CallerUser(ctx, db, node, first)
		if err != nil {
			return err
		}
		if owner.Local || !owner.Administrator || owner.ID == local.ID {
			return errors.New("first paired user was not a separate administrator")
		}
		if err := PairLinkPeer(ctx, db, first, link.DirectionHost); err != nil {
			return err
		}
		retained, err := CallerUser(ctx, db, node, first)
		if err != nil {
			return err
		}
		if retained.ID != owner.ID {
			return errors.New("repeat pairing changed user")
		}
		if err := PairLinkPeer(ctx, db, second, link.DirectionController); err != nil {
			return err
		}
		other, err := CallerUser(ctx, db, node, second)
		if err != nil {
			return err
		}
		if other.Administrator || other.ID == owner.ID {
			return errors.New("second pairing reused user or granted administrator")
		}
		if err := SetLinkDirection(ctx, db, first, link.DirectionHost); err != nil {
			return err
		}
		if _, err := CallerUser(ctx, db, node, first); err == nil {
			return errors.New("narrowed direction retained inbound access")
		}
		if err := SetLinkDirection(ctx, db, first, link.DirectionBoth); err != nil {
			return err
		}
		retained, err = CallerUser(ctx, db, node, first)
		if err != nil {
			return err
		}
		if retained.ID != owner.ID {
			return errors.New("direction change lost membership")
		}
		if err := SetLinkTrust(ctx, db, first, false); err != nil {
			return err
		}
		if _, err := CallerUser(ctx, db, node, first); err == nil {
			return errors.New("revoked peer retained membership")
		}
		users, err := ListUsers(ctx, db)
		if err != nil {
			return err
		}
		if len(users) != 3 {
			return errors.New("revocation deleted user data")
		}
		if err := SetUserAdministrator(ctx, db, local.ID, false); err == nil {
			return errors.New("local administrator was removed")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestLegacyUsersMigrationPreservesPeerAccess(t *testing.T) {
	t.Parallel()
	profile, node, storage := schemaFixture(t)
	ctx := t.Context()
	peer := protocol.NodeID{9}
	schemaExecute(t, storage.Node, "INSERT INTO link_peers(identity,user) VALUES(?1,(SELECT id FROM users WHERE local=1))", peer[:])
	schemaExecute(t, storage.Node, "ALTER TABLE link_peers DROP COLUMN user")
	schemaExecute(t, storage.Node, "ALTER TABLE link_peers DROP COLUMN direction")
	schemaExecute(t, storage.Node, "ALTER TABLE link_peers DROP COLUMN legacy")
	for _, table := range []string{"projects", "sessions", "worktrees", "terminals", "notifications"} {
		schemaExecute(t, storage.Node, "UPDATE "+table+" SET owner=NULL")
	}
	for _, table := range []string{"providers", "roles", "teams", "ssh_profiles", "database_profiles", "turns"} {
		schemaExecute(t, storage.Node, "UPDATE "+table+" SET user=NULL")
	}
	// An old profile had no identity provenance tables. Remove their new
	// singleton owner before constructing that legacy schema; leaving it here
	// would deliberately create a live foreign key to the user being removed.
	for _, table := range []string{"identity_memberships", "identity_sponsorships", "identity_directory", "identity_deliveries", "identity_turn_origins", "identity_storage_origin", "identity_merge_conflicts", "identity_peer_origins", "identity_sponsor_epochs"} {
		schemaExecute(t, storage.Node, "DROP TABLE "+table)
	}
	schemaExecute(t, storage.Node, "DROP TABLE users")
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	migrated := reopenSchemaFixture(t, profile, node)
	err := migrated.Node.Read(ctx, func(db *sql.Conn) error {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		owner, err := CallerUser(ctx, db, node, peer)
		if err != nil {
			return err
		}
		if owner.ID != local.ID {
			return errors.New("legacy peer lost original local-user access")
		}
		directions, err := LinkDirections(ctx, db)
		if err != nil {
			return err
		}
		if len(directions) != 1 || directions[0].Direction != link.DirectionBoth {
			return errors.New("legacy direction was not preserved")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	backups, err := filepath.Glob(filepath.Join(profile, "storage", "backups", "node-before-upgrade-*", "node.sqlite"))
	if err != nil || len(backups) != 1 {
		t.Fatalf("migration backup: %v, %v", backups, err)
	}
	if err := migrated.Close(); err != nil {
		t.Fatal(err)
	}
	_ = reopenSchemaFixture(t, profile, node)
	backups, err = filepath.Glob(filepath.Join(profile, "storage", "backups", "node-before-upgrade-*", "node.sqlite"))
	if err != nil || len(backups) != 1 {
		t.Fatalf("current startup repeated migration: %v, %v", backups, err)
	}
}

func TestHeadlessLegacyDirectionMigration(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := context.Background()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		legacy, explicit := protocol.NodeID{5}, protocol.NodeID{6}
		if err := SetLinkTrust(ctx, db, legacy, true); err != nil {
			return err
		}
		if err := PairLinkPeer(ctx, db, explicit, link.DirectionHost); err != nil {
			return err
		}
		if err := MigrateHeadlessPeers(ctx, db); err != nil {
			return err
		}
		values, err := LinkDirections(ctx, db)
		if err != nil {
			return err
		}
		if len(values) != 2 || values[0].Direction != link.DirectionController || values[1].Direction != link.DirectionHost {
			return errors.New("headless migration changed explicit trust")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
