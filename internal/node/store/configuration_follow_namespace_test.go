package store

import (
	"database/sql"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationFollowNormalizesAutomaticTargets(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx, host := t.Context(), protocol.NodeID{9}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO configuration_follow_targets(user,host,revision,configuration,secrets) VALUES(?1,?2,7,0,0)", user.ID.String(), host[:]); err != nil {
			return err
		}
		if err := EnsureConfigurationTarget(ctx, db, host); err != nil {
			return err
		}
		if err := EnsureConfigurationTarget(ctx, db, host); err != nil {
			return err
		}
		status, err := ReadConfigurationFollow(ctx, db)
		if err == nil && (len(status.Targets) != 1 || !status.Targets[0].Configuration || !status.Targets[0].Secrets || status.Targets[0].Revision != 8) {
			return errors.New("old optional policy did not become stable automatic personal sync")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationFollowRejectsHostLocalReplica(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx, caller := t.Context(), protocol.NodeID{9}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		if err := PairIdentityPeer(ctx, db, node, caller, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationMyDevice, User: &user.ID}); err != nil {
			return err
		}
		_, err = ApplyConfigurationManifest(WithUser(ctx, user), db, node, caller, protocol.ConfigurationManifest{Source: caller, Category: "roles", Revision: 1})
		return err
	}); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("controller changed the host-local namespace: %v", err)
	}
}

func TestConfigurationManifestRejectsUnclassifiedControllerWithoutMaterializing(t *testing.T) {
	storage, node, source, ctx := followFixture(t)
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.Revision = protocol.NewRoleID(), "legacy", "Legacy role", 1
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "UPDATE users SET controller=NULL WHERE id=?1", user.ID.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, followRecord(t, role.ID.String(), 1, role, nil)); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("unclassified source materialized configuration: %v", err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var count int
		if err := db.QueryRowContext(ctx, "SELECT (SELECT count(*) FROM roles WHERE id=?1)+(SELECT count(*) FROM configuration_cache WHERE id=?1)", role.ID.String()).Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			return errors.New("rejected delivery changed configuration")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationCachesKeepSourceIDsSeparateFromFormalRows(t *testing.T) {
	storage, node, source, ctx := followFixture(t)
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.Revision = protocol.NewRoleID(), "reviewer", "Reviewer", 1
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, followRecord(t, role.ID.String(), 1, role, nil)); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		roles, err := ReadRoles(ctx, db)
		if err != nil {
			return err
		}
		if len(roles) != 1 || roles[0].ID != role.ID {
			return errors.New("cache changed source identity")
		}
		var formal int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM roles WHERE id=?1", role.ID.String()).Scan(&formal); err != nil {
			return err
		}
		if formal != 0 {
			return errors.New("cache wrote authoritative role table")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	role.Name, role.Revision = "Updated", 2
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 2, followRecord(t, role.ID.String(), 2, role, nil)); err != nil {
		t.Fatal(err)
	}
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 3); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		old, err := readConfigurationCacheRecord(ctx, db, "roles", role.ID.String(), 1)
		if err != nil || old == nil {
			return errors.New("removal deleted captured role revision")
		}
		current, err := ReadRoles(ctx, db)
		if err != nil {
			return err
		}
		if len(current) != 0 {
			return errors.New("removed role remains current")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
