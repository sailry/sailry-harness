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

func TestLegacyConfigurationMergeRetainsStoredIdentityAndContent(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	controller := protocol.NodeID{7}
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.Revision = protocol.NewRoleID(), "legacy", "Legacy role", 1
	role.Source = &protocol.ConfigurationOrigin{Node: controller, Revision: 1}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		source, err := CreateUser(ctx, db, "Legacy controller")
		if err != nil {
			return err
		}
		target, err := CreateUser(ctx, db, "Surviving user")
		if err != nil {
			return err
		}
		body, err := encode(role)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO roles(id,user,key,body) VALUES(?1,?2,?3,?4)", role.ID.String(), source.ID.String(), configurationKey(source, role.Key), body); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO configuration_follow_sources(user,source,revision) VALUES(?1,?2,1)", source.ID.String(), controller[:]); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, `INSERT INTO configuration_follow_records(user,category,id,source,revision,digest,state,message,body,materialized)
VALUES(?1,'roles',?2,?3,1,'legacy-digest','applied','',?4,1)`, source.ID.String(), role.ID.String(), controller[:], body); err != nil {
			return err
		}
		if _, err := MergeIdentityUsers(ctx, db, source.ID, target.ID); err != nil {
			return err
		}
		roles, err := ReadRoles(WithUser(ctx, target), db)
		if err != nil {
			return err
		}
		if len(roles) != 1 || roles[0].ID != role.ID || roles[0].Name != role.Name || roles[0].Source == nil || *roles[0].Source != *role.Source {
			return errors.New("legacy merge changed stored role content")
		}
		var localID string
		if err := db.QueryRowContext(ctx, "SELECT local_id FROM configuration_follow_records WHERE user=?1 AND category='roles' AND id=?2", target.ID.String(), role.ID.String()).Scan(&localID); err != nil {
			return err
		}
		if localID != role.ID.String() {
			return errors.New("legacy merge lost original identity binding")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestLegacyConfigurationMergeRejectsConflictingBindingsBeforeMutation(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		first, err := CreateUser(ctx, db, "First legacy user")
		if err != nil {
			return err
		}
		second, err := CreateUser(ctx, db, "Second legacy user")
		if err != nil {
			return err
		}
		for index, user := range []protocol.User{first, second} {
			controller := protocol.NodeID{byte(index + 7)}
			if _, err := db.ExecContext(ctx, `INSERT INTO configuration_follow_records(user,category,id,source,revision,digest,state,message,body,materialized)
VALUES(?1,'roles','same-source-id',?2,1,'legacy-digest','pending','pending','{}',0)`, user.ID.String(), controller[:]); err != nil {
				return err
			}
		}
		if _, err := MergeIdentityUsers(ctx, db, first.ID, second.ID); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
			return errors.New("incompatible historical sources merged")
		}
		// Retain the transaction to expose changes made before a late rejection.
		for _, user := range []protocol.User{first, second} {
			current, err := ResolveUser(ctx, db, user.ID)
			if err != nil || current.ID != user.ID || current.State != "active" {
				return errors.New("merge conflict changed a stored principal")
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
