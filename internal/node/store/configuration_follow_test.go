package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func followFixture(t *testing.T) (*Store, protocol.NodeID, protocol.NodeID, context.Context) {
	t.Helper()
	_, node, storage := schemaFixture(t)
	source := protocol.NodeID{7}
	var user protocol.User
	if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := PairLinkPeer(t.Context(), db, source, link.DirectionController); err != nil {
			return err
		}
		var err error
		user, err = CallerUser(t.Context(), db, node, source)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return storage, node, source, WithUser(t.Context(), user)
}
func followRecord(t *testing.T, id string, revision uint64, body any, secret *protocol.Secret) protocol.ConfigurationFollowRecord {
	t.Helper()
	raw, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	value := protocol.ConfigurationFollowRecord{ID: id, Revision: revision, Body: raw, Secret: secret}
	value.Digest, err = configurationDigest(value)
	if err != nil {
		t.Fatal(err)
	}
	return value
}
func applyFollowFixture(t *testing.T, storage *Store, ctx context.Context, node, source protocol.NodeID, category string, revision uint64, records ...protocol.ConfigurationFollowRecord) error {
	t.Helper()
	return storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := ApplyConfigurationManifest(ctx, db, node, source, protocol.ConfigurationManifest{Source: source, Category: category, Revision: revision, Records: records})
		return err
	})
}

func followLocalID(t *testing.T, storage *Store, ctx context.Context, category, id string) string {
	t.Helper()
	var local string
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		local, err = configurationLocalID(ctx, db, category, id)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return local
}

func TestConfigurationFollowPreservesIdentitySecretsAndCapturedCredentials(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	provider := providerFixture()
	provider.Revision = 11
	key := protocol.NewSecret("source-static-fixture")
	record := followRecord(t, provider.ID.String(), provider.Revision, provider, &key)
	if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 1, record); err != nil {
		t.Fatal(err)
	}
	localID, err := protocol.ParseProviderID(followLocalID(t, storage, ctx, "providers", provider.ID.String()))
	if err != nil {
		t.Fatal(err)
	}
	var first protocol.ConversationProvider
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		saved, err := readProviderConfiguration(ctx, db, localID)
		if err != nil {
			return err
		}
		if saved == nil || saved.Provider == nil {
			return errors.New("followed provider absent")
		}
		first = *saved.Provider
		if first.ID == provider.ID || first.ID != localID || first.Source == nil || first.Source.Node != source || first.Source.Revision != 11 || first.Revision != 1 || first.Credential == nil || first.Credential.Node != node {
			return errors.New("source and target identities were not retained separately")
		}
		if err := CheckConfigurationEditable(ctx, db, "providers", localID.String()); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return errors.New("followed provider is editable")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	// Retry an unchanged manifest without resending secret content.
	metadata := record
	metadata.Body, metadata.Secret = nil, nil
	if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 1, metadata); err != nil {
		t.Fatal(err)
	}
	provider.Name = "Updated"
	provider.Revision++
	record = followRecord(t, provider.ID.String(), provider.Revision, provider, nil)
	if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 2, record); err != nil {
		t.Fatal(err)
	}
	if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 3); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		saved, err := readProviderConfiguration(ctx, db, localID)
		if err != nil {
			return err
		}
		if saved.Listed {
			return errors.New("source deletion retained live copy")
		}
		auth := saved.Authorizations[first.Credential.ID]
		if auth.Key == nil || auth.Key.Expose() != key.Expose() {
			return errors.New("source deletion destroyed captured credential")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
func TestConfigurationFollowOwnsReceivingRoleKey(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	local := protocol.DefaultRoleProfile()
	local.ID, local.Key, local.Name = protocol.NewRoleID(), "reviewer", "Local reviewer"
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error { _, err := putRole(ctx, db, local, 0); return err }); err != nil {
		t.Fatal(err)
	}
	incoming := local
	incoming.ID = protocol.NewRoleID()
	incoming.Name = "Controller reviewer"
	incoming.Revision = 5
	record := followRecord(t, incoming.ID.String(), 5, incoming, nil)
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, record); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		status, err := ReadConfigurationFollow(ctx, db)
		if err != nil {
			return err
		}
		if len(status.Issues) != 0 {
			return errors.New("receiving namespace role was left in conflict")
		}
		roles, err := ReadRoles(ctx, db)
		if err != nil {
			return err
		}
		if len(roles) != 1 || roles[0].ID != local.ID || roles[0].Name != incoming.Name || roles[0].Source == nil {
			return errors.New("source did not own its receiving role namespace")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 2); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		roles, err := ReadRoles(ctx, db)
		if err != nil {
			return err
		}
		if len(roles) != 0 {
			return errors.New("source deletion did not remove its receiving copy")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
func TestConfigurationFollowRetainsGoodCopyDuringConflictAndDetach(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name = protocol.NewRoleID(), "reviewer", "Reviewer"
	role.Revision = 3
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, followRecord(t, role.ID.String(), 3, role, nil)); err != nil {
		t.Fatal(err)
	}
	localID := followLocalID(t, storage, ctx, "roles", role.ID.String())
	role.Key = "invalid key"
	role.Revision++
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 2, followRecord(t, role.ID.String(), 4, role, nil)); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := CheckConfigurationEditable(ctx, db, "roles", localID); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return errors.New("failed update unlocked previous good copy")
		}
		status, err := ReadConfigurationFollow(ctx, db)
		if err != nil {
			return err
		}
		if err := SetConfigurationSource(ctx, db, node, source, nil, status.Revision); err != nil {
			return err
		}
		return DetachConfigurationCopies(ctx, db, "")
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		if err := CheckConfigurationEditable(ctx, db, "roles", localID); err != nil {
			return err
		}
		value, err := readUserConfiguration[protocol.RoleProfile](ctx, db, "roles", localID)
		if err != nil {
			return err
		}
		if value == nil || value.Key != "reviewer" || value.Source != nil {
			return errors.New("detach changed last good copy")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
func TestConfigurationFollowRejectsReplayAndCallerSpoofing(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name = protocol.NewRoleID(), "reviewer", "Reviewer"
	role.Revision = 1
	record := followRecord(t, role.ID.String(), 1, role, nil)
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, record); err != nil {
		t.Fatal(err)
	}
	role.Name = "Reused revision"
	changed := followRecord(t, role.ID.String(), 2, role, nil)
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, changed); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
		t.Fatalf("same-version changed manifest=%v", err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := ApplyConfigurationManifest(ctx, db, node, protocol.NodeID{9}, protocol.ConfigurationManifest{Source: source, Category: "roles", Revision: 2, Records: []protocol.ConfigurationFollowRecord{record}})
		return err
	}); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("spoof source=%v", err)
	}
}
func TestConfigurationFollowWaitsForExactPluginVersion(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	info := pluginTestInfo("follow-fixture", 'a')
	info.Summary.Revision = 1
	info.Summary.Enabled = true
	record := followRecord(t, info.Summary.Name, 1, info, nil)
	if err := applyFollowFixture(t, storage, ctx, node, source, "plugins", 1, record); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		status, err := ReadConfigurationFollow(ctx, db)
		if err != nil {
			return err
		}
		if len(status.Issues) != 1 || status.Issues[0].State != "pending" {
			return errors.New("missing package was not pending")
		}
		return CacheConfigurationPackage(ctx, db, info, protocol.PluginMcpConfiguration{Servers: map[string]protocol.PluginMcpServer{}})
	}); err != nil {
		t.Fatal(err)
	}
	if err := applyFollowFixture(t, storage, ctx, node, source, "plugins", 1, record); err != nil {
		t.Fatal(err)
	}
	settings := pluginFollowSettings{Package: plugins.PackageReference(info), Values: map[string]json.RawMessage{}, Allowed: map[string]bool{}}
	if err := applyFollowFixture(t, storage, ctx, node, source, "plugin_settings", 1, followRecord(t, info.Summary.Name, 1, settings, nil)); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		saved, err := ReadPlugin(ctx, db, info.Summary.Name)
		if err != nil {
			return err
		}
		if saved == nil || saved.Summary.Digest != info.Summary.Digest || saved.Summary.Source == nil {
			return errors.New("plugin digest/source was not retained")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationFollowResumesOnlyOriginalSource(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	value := protocol.DefaultRoleProfile()
	value.ID, value.Key, value.Name, value.Revision = protocol.NewRoleID(), "resume-fixture", "Resume", 1
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, followRecord(t, value.ID.String(), 1, value, nil)); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		status, err := ReadConfigurationFollow(ctx, db)
		if err != nil {
			return err
		}
		if err := SetConfigurationSource(ctx, db, node, source, nil, status.Revision); err != nil {
			return err
		}
		status, err = ReadConfigurationFollow(ctx, db)
		if err != nil {
			return err
		}
		return SetConfigurationSource(ctx, db, node, source, &source, status.Revision)
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationFollowAdoptsUntouchedBundledPlugin(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	info := pluginTestInfo("bundled-follow", 'b')
	info.Origin = &protocol.PluginOrigin{Kind: "bundled"}
	info.Summary.Enabled = true
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		info, err = InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		return CacheConfigurationPackage(ctx, db, info, protocol.PluginMcpConfiguration{Servers: map[string]protocol.PluginMcpServer{}})
	}); err != nil {
		t.Fatal(err)
	}
	if err := applyFollowFixture(t, storage, ctx, node, source, "plugins", 1, followRecord(t, info.Summary.Name, 1, info, nil)); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		status, err := ReadConfigurationFollow(ctx, db)
		if err != nil {
			return err
		}
		if len(status.Issues) != 0 {
			return errors.New("pristine bundled installation was treated as a personal conflict")
		}
		return CheckConfigurationEditable(ctx, db, "plugins", info.Summary.Name)
	}); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("followed bundled package must be readonly: %v", err)
	}
}
