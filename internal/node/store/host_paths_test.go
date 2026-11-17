package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestControllerFileLocationsRemainOnTheirOwningNode(t *testing.T) {
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		path := `C:\private\archive.sqlite`
		profile := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Revision: 1, Name: "Windows database",
			Connection: protocol.DatabaseConnection{Kind: "sqlite", Data: protocol.DatabaseConnectionSqlitePayload{Path: path}},
			Location:   &protocol.HostPath{Node: controller, Path: path},
		}
		if err := validateDatabaseProfile(profile, nil, node); err != nil {
			return fmt.Errorf("controller imposed its own filesystem rules on another Node: %w", err)
		}
		record := followRecord(t, profile.ID.String(), profile.Revision, profile, nil)
		if _, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
			Source: controller, Category: "database_profiles", Revision: 1, Records: []protocol.ConfigurationFollowRecord{record},
		}); err != nil {
			return err
		}
		found, err := ReadDatabase(ctx, db, profile.ID)
		if err != nil || found.Location == nil || found.Location.Node != controller || found.Location.Path != path {
			return fmt.Errorf("cached database location was rebound to executor: %w", err)
		}
		_, err = PrepareDatabase(ctx, db, t.TempDir(), protocol.Command{Kind: "check_database", Data: protocol.CommandCheckDatabasePayload{
			Profile: profile.ID, ExpectedRevision: profile.Revision,
		}})
		if !providerFixtureFault(err, protocol.ErrorCodeWrongTarget) {
			return fmt.Errorf("executor attempted another Node's file: %v", err)
		}
		ssh := protocol.DefaultSshProfile()
		ssh.ID, ssh.Revision, ssh.Host, ssh.Port, ssh.Username = protocol.NewSshID(), 1, "ssh.example.test", 22, "fixture"
		ssh.Authentication, ssh.AuthenticationNode = protocol.SshAuthenticationAgent, &controller
		record = followRecord(t, ssh.ID.String(), ssh.Revision, ssh, nil)
		if _, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
			Source: controller, Category: "ssh_profiles", Revision: 1, Records: []protocol.ConfigurationFollowRecord{record},
		}); err != nil {
			return err
		}
		if _, _, err := ReadSsh(ctx, db, ssh.ID, ssh.Revision); !providerFixtureFault(err, protocol.ErrorCodeWrongTarget) {
			return fmt.Errorf("executor attempted its own SSH_AUTH_SOCK for a controller-bound agent: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCachedSshTrustSurvivesConfigurationUpdates(t *testing.T) {
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		profile := protocol.DefaultSshProfile()
		profile.ID, profile.Revision, profile.Name = protocol.NewSshID(), 1, "Initial"
		profile.Host, profile.Port, profile.Username, profile.Authentication = "ssh.example.test", 22, "fixture", protocol.SshAuthenticationPassword
		credential := protocol.NewSecret(`{"kind":"password","password":"fixture-password"}`)
		record := followRecord(t, profile.ID.String(), 1, profile, &credential)
		apply := func(record protocol.ConfigurationFollowRecord, revision uint64) error {
			_, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{
				Source: controller, Category: "ssh_profiles", Revision: revision, Records: []protocol.ConfigurationFollowRecord{record},
			})
			return err
		}
		if err := apply(record, 1); err != nil {
			return err
		}
		key := protocol.SshHostKey{Algorithm: "ssh-ed25519", Fingerprint: "SHA256:fixture"}
		if err := saveSshTrust(ctx, db, profile, key); err != nil {
			return err
		}
		profile.Name, profile.Revision = "Renamed", 2
		if err := apply(followRecord(t, profile.ID.String(), 2, profile, &credential), 2); err != nil {
			return err
		}
		cached, _, err := ReadSsh(ctx, db, profile.ID, profile.Revision)
		if err != nil || cached.HostKey == nil || *cached.HostKey != key {
			return fmt.Errorf("configuration update discarded local SSH trust: %w", err)
		}
		profile.Host, profile.Revision = "different.example.test", 3
		if err := apply(followRecord(t, profile.ID.String(), 3, profile, &credential), 3); err != nil {
			return err
		}
		cached, _, err = ReadSsh(ctx, db, profile.ID, profile.Revision)
		if err != nil || cached.HostKey != nil {
			return fmt.Errorf("SSH trust leaked to a different endpoint: %w", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestSshProfileEditsPreserveAuthenticationLocation(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		remote := protocol.NodeID{2}
		profile := protocol.DefaultSshProfile()
		profile.ID, profile.Name = protocol.NewSshID(), "Remote key"
		profile.Host, profile.Port, profile.Username = "ssh.example.test", 22, "fixture"
		profile.Authentication, profile.AuthenticationNode = protocol.SshAuthenticationKeyPath, &remote
		credential := protocol.SshCredential{Kind: "key_path", Data: &protocol.SshCredentialKeyPathPayload{
			Path: protocol.NewSecret("/remote/private/key"), Location: &protocol.HostPath{Node: remote, Path: "/remote/private/key"},
		}}
		save := func(credential *protocol.SshCredential) (protocol.SshProfile, error) {
			output, _, err := ExecuteSshCommand(ctx, db, protocol.Command{Kind: "save_ssh", Data: &protocol.CommandSaveSshPayload{
				Profile: profile, Credential: credential, ExpectedRevision: profile.Revision,
			}})
			if err != nil {
				return protocol.SshProfile{}, err
			}
			return output.Data.(protocol.SshProfile), nil
		}
		var err error
		profile, err = save(&credential)
		if err != nil {
			return err
		}
		if _, _, err := ReadSsh(ctx, db, profile.ID, profile.Revision); !providerFixtureFault(err, protocol.ErrorCodeWrongTarget) {
			return fmt.Errorf("remote authentication was executed locally: %v", err)
		}
		profile.Name, profile.AuthenticationNode = "Renamed", nil
		profile, err = save(nil)
		if err != nil || profile.AuthenticationNode == nil || *profile.AuthenticationNode != remote {
			return fmt.Errorf("renaming moved the existing key: %v", err)
		}
		other := protocol.NodeID{3}
		profile.AuthenticationNode = &other
		if _, err := save(nil); err == nil {
			return fmt.Errorf("existing key accepted another authentication Node")
		}
		_, _, err = ExecuteSshCommand(ctx, db, protocol.Command{Kind: "remove_ssh", Data: &protocol.CommandRemoveSshPayload{
			Profile: profile.ID, ExpectedRevision: profile.Revision,
		}})
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestDatabaseDraftRequiresItsFileLocation(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		remote := protocol.NodeID{2}
		profile := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: "Remote file", Location: &protocol.HostPath{Node: remote, Path: "/remote/archive.sqlite"},
			Connection: protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{Path: "/remote/archive.sqlite"}}}
		output, _, err := ExecuteDatabaseCommand(ctx, db, protocol.Command{Kind: "save_database", Data: &protocol.CommandSaveDatabasePayload{Profile: profile}})
		if err != nil {
			return err
		}
		profile = output.Data.(protocol.DatabaseProfile)
		profile.Location = nil
		input := &protocol.CommandTestDatabasePayload{Profile: profile, ExpectedRevision: profile.Revision}
		_, err = PrepareDatabase(ctx, db, storage.Profile, protocol.Command{Kind: "test_database", Data: input})
		if !providerFixtureFault(err, protocol.ErrorCodeWrongTarget) {
			return fmt.Errorf("saved remote path was rebound to this Node: %v", err)
		}
		input.Profile.ID, input.Profile.Revision, input.ExpectedRevision = protocol.NewDatabaseID(), 0, 0
		_, err = PrepareDatabase(ctx, db, storage.Profile, protocol.Command{Kind: "test_database", Data: input})
		if !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("an unknown path location defaulted to this Node: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
