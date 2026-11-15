package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderUsersAndHostModels(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		other, err := CreateUser(ctx, db, "Other")
		if err != nil {
			return err
		}
		other.Administrator = false
		if err := SetUserAdministrator(ctx, db, other.ID, false); err != nil {
			return err
		}
		admin, err := CreateUser(ctx, db, "Administrator")
		if err != nil {
			return err
		}
		admin.Administrator = true
		if err := SetUserAdministrator(ctx, db, admin.ID, true); err != nil {
			return err
		}
		ownCtx, otherCtx, adminCtx := WithUser(ctx, owner), WithUser(ctx, other), WithUser(ctx, admin)
		save := func(ctx context.Context, provider protocol.ConversationProvider, secret string) (protocol.ConversationProvider, error) {
			key := protocol.NewSecret(secret)
			output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, ExpectedRevision: provider.Revision, Secret: &key}}}, nil)
			if err != nil {
				return protocol.ConversationProvider{}, err
			}
			return output.Data.(protocol.ConversationProvider), nil
		}
		private, err := save(ownCtx, providerFixture(), "owner-key")
		if err != nil {
			return err
		}
		if private.Owner == nil || *private.Owner != owner.ID {
			return errors.New("provider owner is missing")
		}
		for _, scoped := range []context.Context{otherCtx, adminCtx} {
			if values, err := providerList(scoped, db); err != nil || len(values) != 0 {
				return fmt.Errorf("private provider visibility: %v", err)
			}
			if _, err := save(scoped, private, "replacement-key"); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
				return fmt.Errorf("cross-user provider overwrite: %v", err)
			}
			if _, err := resolveProviderKey(scoped, db, node, *private.Credential, private.ID); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
				return fmt.Errorf("private key resolution: %v", err)
			}
		}
		shared := providerFixture()
		shared.Shared = true
		shared, err = save(ownCtx, shared, "host-key")
		if err != nil {
			return err
		}
		values, quotas, err := ReadProviderSnapshot(otherCtx, db, node)
		if err != nil || len(values) != 1 || values[0].ID != shared.ID || len(quotas) != 0 {
			return fmt.Errorf("host model snapshot: %v", err)
		}
		if key, err := resolveProviderKey(otherCtx, db, node, *shared.Credential, shared.ID); err != nil || key.Expose() != "host-key" {
			return fmt.Errorf("host model execution: %v", err)
		}
		if _, err := ReadProviderCommand(otherCtx, db, node, protocol.Command{Kind: "read_provider_key", Data: &protocol.CommandReadProviderKeyPayload{Provider: shared.ID, ExpectedRevision: shared.Revision}}); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("host key disclosure: %v", err)
		}
		if _, err := save(otherCtx, shared, "replacement-key"); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("host provider mutation: %v", err)
		}
		if _, _, err := exportCredential(otherCtx, db, node, *shared.Credential, shared); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("host key export: %v", err)
		}
		if _, err := save(adminCtx, shared, "rotated-host-key"); err != nil {
			return err
		}
		// Captured revisions retain their old key through rotation and visibility edits.
		latest, err := currentProvider(adminCtx, db, shared.ID, nil)
		if err != nil {
			return err
		}
		changed := *latest.Provider
		changed.Shared = false
		if _, err := save(adminCtx, changed, "private-host-key"); err != nil {
			return err
		}
		if current, err := readProviderConfiguration(otherCtx, db, shared.ID); err != nil || current != nil {
			return fmt.Errorf("narrowed host model remains visible: %v", err)
		}
		shared.Shared = true
		if key, err := resolveCapturedProviderKey(otherCtx, db, node, *shared.Credential, shared); err != nil || key.Expose() != "host-key" {
			return fmt.Errorf("captured host key was lost: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestUserConfigurationPartitions(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		other, err := CreateUser(ctx, db, "Other")
		if err != nil {
			return err
		}
		ownCtx, otherCtx := WithUser(ctx, owner), WithUser(ctx, other)
		role := protocol.RoleProfile{ID: protocol.NewRoleID(), Key: "fixture", Name: "Owner role", Skills: []string{}}
		role, err = putRole(ownCtx, db, role, 0)
		if err != nil {
			return err
		}
		ownTeam := protocol.TeamProfile{ID: protocol.NewTeamID(), Key: "fixture", Name: "Owner team", Members: []protocol.RoleID{role.ID}}
		ownTeam, err = putTeam(ownCtx, db, ownTeam, 0)
		if err != nil {
			return err
		}
		if roles, err := ReadRoles(otherCtx, db); err != nil || len(roles) != 0 {
			return fmt.Errorf("role visibility: %v", err)
		}
		if teams, err := ReadTeams(otherCtx, db); err != nil || len(teams) != 0 {
			return fmt.Errorf("team visibility: %v", err)
		}
		if _, err := putRole(otherCtx, db, role, role.Revision); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("role overwrite: %v", err)
		}
		if _, err := putTeam(otherCtx, db, ownTeam, ownTeam.Revision); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("team overwrite: %v", err)
		}
		otherRole := role
		otherRole.ID, otherRole.Revision = protocol.NewRoleID(), 0
		otherRole, err = putRole(otherCtx, db, otherRole, 0)
		if err != nil {
			return err
		}
		otherTeam := ownTeam
		otherTeam.ID, otherTeam.Revision = protocol.NewTeamID(), 0
		if _, err := putTeam(otherCtx, db, otherTeam, 0); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("foreign team member: %v", err)
		}
		otherTeam.Members = []protocol.RoleID{otherRole.ID}
		if _, err := putTeam(otherCtx, db, otherTeam, 0); err != nil {
			return err
		}
		settings := protocol.DefaultTeamSettings()
		settings.ChildConcurrency++
		if _, _, err := ExecuteTeamCommand(otherCtx, db, protocol.Command{Kind: "save_team_settings", Data: &settings}); err != nil {
			return err
		}
		if current, err := ReadTeamSettings(ownCtx, db); err != nil || current.Revision != 0 {
			return fmt.Errorf("team settings crossed users: %v", err)
		}
		password := "profile-password"
		ssh, err := databaseFixtureSsh(ownCtx, db, protocol.SshProfile{ID: protocol.NewSshID(), Name: "Owner SSH", Host: "bastion.example", Port: 22, Username: "deploy", Authentication: protocol.SshAuthenticationPassword}, 0, &password)
		if err != nil {
			return err
		}
		if _, _, err := ReadSsh(otherCtx, db, ssh.ID, ssh.Revision); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("SSH credential visibility: %v", err)
		}
		if _, err := databaseFixtureSsh(otherCtx, db, ssh, ssh.Revision, &password); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("SSH overwrite: %v", err)
		}
		profile := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: "Owner database", Connection: databaseFixtureNetwork("database.example")}
		if _, _, err := databaseFixtureSave(ownCtx, db, profile, 0, &password); err != nil {
			return err
		}
		if current, err := ReadDatabase(otherCtx, db, profile.ID); err != nil || current != nil {
			return fmt.Errorf("database visibility: %v", err)
		}
		if _, _, err := databaseFixtureSave(otherCtx, db, profile, 0, &password); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("database overwrite: %v", err)
		}
		if _, err := databaseTunnelProfile(otherCtx, db, databaseFixtureTunnel(ssh.ID)); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("foreign SSH tunnel: %v", err)
		}
		provider := providerFixture()
		output, _, err := ExecuteProviderCommand(otherCtx, db, node, protocol.Request{Command: protocol.Command{Kind: "put_provider", Data: &protocol.CommandPutProviderPayload{Provider: provider}}}, nil)
		if err != nil {
			return err
		}
		saved := output.Data.(protocol.ConversationProvider)
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model = saved.ID, saved.DefaultModel
		if _, err := SaveDefaults(otherCtx, db, node, protocol.Defaults{Config: &config}); err != nil {
			return err
		}
		if current, err := ReadDefaults(ownCtx, db); err != nil || current.Config != nil {
			return fmt.Errorf("defaults crossed users: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestLegacyConfigurationUsersRemainLocal(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		other, err := CreateUser(ctx, db, "Other")
		if err != nil {
			return err
		}
		legacy := providerFixture()
		saved := providerConfiguration{ID: legacy.ID, Provider: &legacy, Listed: true, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
		body, err := encode(saved)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO providers(id,body) VALUES(?1,?2)", legacy.ID.String(), body); err != nil {
			return err
		}
		for range 2 {
			if err := InitializeConfigurationUsers(ctx, db); err != nil {
				return err
			}
		}
		owner, present, err := configurationOwner(ctx, db, "providers", legacy.ID.String())
		if err != nil || !present || owner != local.ID {
			return fmt.Errorf("legacy owner changed: %v", err)
		}
		if values, _, err := ReadProviderSnapshot(WithUser(ctx, local), db, node); err != nil || len(values) != 1 || values[0].ID != legacy.ID || values[0].Shared {
			return fmt.Errorf("legacy local visibility: %v", err)
		}
		if values, _, err := ReadProviderSnapshot(WithUser(ctx, other), db, node); err != nil || len(values) != 0 {
			return fmt.Errorf("legacy configuration exposed: %v", err)
		}
		return validateConfigurationUsers(ctx, db)
	})
	if err != nil {
		t.Fatal(err)
	}
}
