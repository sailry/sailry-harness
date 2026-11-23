package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestControllerFileLocationsDoNotAppearOnExecutionNode(t *testing.T) {
	t.Parallel()
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
		if err := seedHistoricalConfiguration(ctx, db, "database_profiles", profile.ID.String(), profile.Revision, profile); err != nil {
			return err
		}
		found, err := ReadDatabase(ctx, db, profile.ID)
		if err != nil || found != nil {
			return fmt.Errorf("cached database location appeared on executor: %v", err)
		}
		_, err = PrepareDatabase(ctx, db, t.TempDir(), protocol.Command{Kind: "check_database", Data: protocol.CommandCheckDatabasePayload{
			Profile: profile.ID, ExpectedRevision: profile.Revision,
		}})
		if !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("executor attempted another Node's file: %v", err)
		}
		ssh := protocol.DefaultSshProfile()
		ssh.ID, ssh.Revision, ssh.Host, ssh.Port, ssh.Username = protocol.NewSshID(), 1, "ssh.example.test", 22, "fixture"
		ssh.Authentication, ssh.AuthenticationNode = protocol.SshAuthenticationAgent, &controller
		if err := seedHistoricalConfiguration(ctx, db, "ssh_profiles", ssh.ID.String(), ssh.Revision, ssh); err != nil {
			return err
		}
		if _, _, err := ReadSsh(ctx, db, ssh.ID, ssh.Revision); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("executor attempted its own SSH_AUTH_SOCK for a controller-bound agent: %v", err)
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
