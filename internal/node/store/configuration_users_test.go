package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderUsersIgnoreLegacySharing(t *testing.T) {
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
		if err := setAdministrator(ctx, db, other.ID, false); err != nil {
			return err
		}
		admin, err := CreateUser(ctx, db, "Administrator")
		if err != nil {
			return err
		}
		admin.Administrator = true
		if err := setAdministrator(ctx, db, admin.ID, true); err != nil {
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
		if shared.Shared {
			return errors.New("save retained the legacy sharing flag")
		}
		// Reproduce an existing row and an admitted capture from the old policy.
		legacy, err := readProviderConfiguration(ownCtx, db, shared.ID)
		if err != nil {
			return err
		}
		legacy.Provider.Shared = true
		body, err := encode(legacy)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE providers SET shared=1,body=?2 WHERE id=?1", shared.ID.String(), body); err != nil {
			return err
		}
		shared.Shared = true
		for _, scoped := range []context.Context{otherCtx, adminCtx} {
			if values, err := providerList(scoped, db); err != nil || len(values) != 0 {
				return fmt.Errorf("legacy provider list crossed users: %v", err)
			}
			if values, quotas, err := ReadProviderSnapshot(scoped, db, node); err != nil || len(values) != 0 || len(quotas) != 0 {
				return fmt.Errorf("legacy provider snapshot crossed users: %v", err)
			}
			if values, err := providerCredentials(scoped, db); err != nil || len(values) != 0 {
				return fmt.Errorf("legacy credential metadata crossed users: %v", err)
			}
			if current, err := readProviderConfiguration(scoped, db, shared.ID); err != nil || current != nil {
				return fmt.Errorf("legacy provider read crossed users: %v", err)
			}
			if _, err := resolveProviderKey(scoped, db, node, *shared.Credential, shared.ID); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
				return fmt.Errorf("legacy key resolution crossed users: %v", err)
			}
			if _, err := ReadProviderCommand(scoped, db, node, protocol.Command{Kind: "read_provider_key", Data: &protocol.CommandReadProviderKeyPayload{Provider: shared.ID, ExpectedRevision: shared.Revision}}); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
				return fmt.Errorf("legacy key disclosure: %v", err)
			}
			if _, err := save(scoped, shared, "replacement-key"); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
				return fmt.Errorf("legacy provider overwrite: %v", err)
			}
			if _, _, err := ExecuteProviderCommand(scoped, db, node, protocol.Request{Command: protocol.Command{Kind: "remove_provider", Data: &protocol.CommandRemoveProviderPayload{Provider: shared.ID, ExpectedRevision: shared.Revision}}}, nil); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
				return fmt.Errorf("legacy provider removal: %v", err)
			}
			if _, _, err := exportCredential(scoped, db, node, *shared.Credential, shared); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
				return fmt.Errorf("legacy key export: %v", err)
			}
		}
		current, err := currentProvider(ownCtx, db, shared.ID, nil)
		if err != nil {
			return err
		}
		if current.Provider.Shared || current.Provider.Owner == nil || *current.Provider.Owner != owner.ID {
			return errors.New("legacy owner projection is not personal")
		}
		if key, err := resolveProviderKey(ownCtx, db, node, *shared.Credential, shared.ID); err != nil || key.Expose() != "host-key" {
			return fmt.Errorf("owner key was lost: %v", err)
		}
		manifest, err := ExportConfigurationManifest(ownCtx, db, node, protocol.NodeID{7}, "providers", true)
		if err != nil {
			return err
		}
		if len(manifest.Records) != 2 {
			return errors.New("legacy provider was lost from owner export")
		}
		for _, record := range manifest.Records {
			var provider protocol.ConversationProvider
			if err := decodeConfigurationRecord(record, &provider); err != nil {
				return err
			}
			if provider.Shared || provider.Owner != nil || provider.Source != nil {
				return errors.New("export retained source ownership or sharing")
			}
		}
		var columnShared, bodyShared bool
		if err := db.QueryRowContext(ctx, "SELECT shared,COALESCE(json_extract(body,'$.provider.shared'),0) FROM providers WHERE id=?1", shared.ID.String()).Scan(&columnShared, &bodyShared); err != nil {
			return err
		}
		if !columnShared || !bodyShared {
			return errors.New("reading legacy configuration rewrote stored data")
		}
		changed, err := save(ownCtx, shared, "private-host-key")
		if err != nil {
			return err
		}
		if changed.Shared {
			return errors.New("owner save retained sharing")
		}
		if err := db.QueryRowContext(ctx, "SELECT shared,COALESCE(json_extract(body,'$.provider.shared'),0) FROM providers WHERE id=?1", shared.ID.String()).Scan(&columnShared, &bodyShared); err != nil {
			return err
		}
		if columnShared || bodyShared {
			return errors.New("owner save did not clear sharing")
		}
		// Only the immutable legacy capture may recover the pre-rotation key.
		if key, err := resolveCapturedProviderKey(otherCtx, db, node, *shared.Credential, shared); err != nil || key.Expose() != "host-key" {
			return fmt.Errorf("captured host key was lost: %v", err)
		}
		uncaptured := shared
		uncaptured.Shared = false
		if _, err := resolveCapturedProviderKey(otherCtx, db, node, *shared.Credential, uncaptured); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("uncaptured provider recovered another user's key: %v", err)
		}
		uncaptured = shared
		uncaptured.Owner = &other.ID
		if _, err := resolveCapturedProviderKey(otherCtx, db, node, *shared.Credential, uncaptured); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("captured owner mismatch accepted: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestProviderCommandsCannotEnableSharing(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := CreateUser(ctx, db, "Personal controller")
		if err != nil {
			return err
		}
		if err := setAdministrator(ctx, db, user.ID, false); err != nil {
			return err
		}
		user.Administrator = false
		personal := WithUser(ctx, user)
		for _, kind := range []string{"put_provider", "save_provider"} {
			provider := providerFixture()
			provider.Shared = true
			var payload any = &protocol.CommandPutProviderPayload{Provider: provider}
			if kind == "save_provider" {
				key := protocol.NewSecret("personal-fixture-key")
				payload = &protocol.CommandSaveProviderPayload{Provider: provider, Secret: &key}
			}
			output, changes, err := ExecuteProviderCommand(personal, db, node, protocol.Request{Command: protocol.Command{Kind: kind, Data: payload}}, nil)
			if err != nil {
				return err
			}
			saved := output.Data.(protocol.ConversationProvider)
			if saved.Shared || saved.Owner == nil || *saved.Owner != user.ID {
				return fmt.Errorf("%s did not return a personal provider", kind)
			}
			if len(changes) != 1 || changes[0].Data.(protocol.ConversationProvider).Shared {
				return fmt.Errorf("%s emitted a shared provider", kind)
			}
			var columnShared, bodyShared bool
			if err := db.QueryRowContext(ctx, "SELECT shared,COALESCE(json_extract(body,'$.provider.shared'),0) FROM providers WHERE id=?1", saved.ID.String()).Scan(&columnShared, &bodyShared); err != nil {
				return err
			}
			if columnShared || bodyShared {
				return fmt.Errorf("%s persisted sharing", kind)
			}
		}
		if values, err := providerList(ctx, db); err != nil || len(values) != 0 {
			return fmt.Errorf("local administrator saw personal controller providers: %v", err)
		}
		return nil
	}); err != nil {
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
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
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

// setAdministrator changes a fixture user's role directly; no command does.
func setAdministrator(ctx context.Context, db *sql.Conn, id protocol.UserID, administrator bool) error {
	_, err := db.ExecContext(ctx, "UPDATE users SET administrator=?2 WHERE id=?1", id.String(), administrator)
	return err
}
