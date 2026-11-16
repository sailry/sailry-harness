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

func TestConfigurationFollowSeparatesControllerNamespaces(t *testing.T) {
	t.Parallel()
	storage, node, source, first := followFixture(t)
	secondSource := protocol.NodeID{8}
	var second, local context.Context
	if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := PairLinkPeer(t.Context(), db, secondSource, link.DirectionController); err != nil {
			return err
		}
		user, err := CallerUser(t.Context(), db, node, secondSource)
		if err != nil {
			return err
		}
		second = WithUser(t.Context(), user)
		owner, err := LocalUser(t.Context(), db)
		local = WithUser(t.Context(), owner)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	provider := providerFixture()
	provider.Name, provider.Revision = "Host-local provider", 1
	if err := storage.Node.Write(local, func(db *sql.Conn) error {
		return saveProviderConfiguration(local, db, providerConfiguration{ID: provider.ID, Provider: &provider, Listed: true})
	}); err != nil {
		t.Fatal(err)
	}
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.Revision = protocol.NewRoleID(), "controller", "Controller role", 1
	role.Model = &protocol.RoleModel{Provider: provider.ID, Model: provider.DefaultModel}
	team := protocol.DefaultTeamProfile()
	team.ID, team.Key, team.Name, team.Revision = protocol.NewTeamID(), "controller", "Controller team", 1
	team.Members, team.Model = []protocol.RoleID{role.ID}, role.Model
	ssh := protocol.SshProfile{ID: protocol.NewSshID(), Revision: 1, Name: "Controller SSH", Host: "ssh.example.test", Port: 22, Username: "controller", Authentication: protocol.SshAuthenticationPassword}
	database := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Revision: 1, Name: "Controller database", Connection: databaseFixtureTunnel(ssh.ID)}
	credential, err := json.Marshal(protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("ssh-fixture-key")}})
	if err != nil {
		t.Fatal(err)
	}
	sshKey, databaseKey := protocol.NewSecret(string(credential)), protocol.NewSecret("database-fixture-key")
	localIDs := map[string]string{}
	for index, fixture := range []struct {
		caller protocol.NodeID
		ctx    context.Context
	}{{source, first}, {secondSource, second}} {
		provider.Name = []string{"Controller A", "Controller B"}[index]
		key := protocol.NewSecret([]string{"controller-a-key", "controller-b-key"}[index])
		if err := applyFollowFixture(t, storage, fixture.ctx, node, fixture.caller, "providers", 1, followRecord(t, provider.ID.String(), 1, provider, &key)); err != nil {
			t.Fatal(err)
		}
		if err := applyFollowFixture(t, storage, fixture.ctx, node, fixture.caller, "roles", 1, followRecord(t, role.ID.String(), 1, role, nil)); err != nil {
			t.Fatal(err)
		}
		if err := applyFollowFixture(t, storage, fixture.ctx, node, fixture.caller, "teams", 1, followRecord(t, team.ID.String(), 1, team, nil)); err != nil {
			t.Fatal(err)
		}
		for _, entry := range []struct {
			category, id string
			body         any
			secret       *protocol.Secret
		}{{"ssh_profiles", ssh.ID.String(), ssh, &sshKey}, {"database_profiles", database.ID.String(), database, &databaseKey}} {
			if err := applyFollowFixture(t, storage, fixture.ctx, node, fixture.caller, entry.category, 1, followRecord(t, entry.id, 1, entry.body, entry.secret)); err != nil {
				t.Fatal(err)
			}
		}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, provider.DefaultModel, protocol.WorkModeCode, protocol.PermissionProject
		config.Credential = &protocol.CredentialRef{Node: fixture.caller, ID: protocol.NewCredentialID()}
		if err := applyFollowFixture(t, storage, fixture.ctx, node, fixture.caller, "defaults", 1, followRecord(t, "default", 1, protocol.Defaults{Revision: 1, Config: &config}, nil)); err != nil {
			t.Fatal(err)
		}
		id := followLocalID(t, storage, fixture.ctx, "providers", provider.ID.String())
		localIDs[provider.Name] = id
		if err := storage.Node.Read(fixture.ctx, func(db *sql.Conn) error {
			mapped, err := protocol.ParseProviderID(id)
			if err != nil {
				return err
			}
			providers, err := providerConfigurations(fixture.ctx, db)
			if err != nil {
				return err
			}
			if len(providers) != 1 || providers[0].Provider.Name != provider.Name || providers[0].ID != mapped {
				return errors.New("controller saw another namespace's provider")
			}
			credential := providers[0].Provider.Credential
			if credential == nil || providers[0].Authorizations[credential.ID].Key == nil || providers[0].Authorizations[credential.ID].Key.Expose() != key.Expose() {
				return errors.New("controller credential escaped its personal namespace")
			}
			roles, err := ReadRoles(fixture.ctx, db)
			if err != nil {
				return err
			}
			teams, err := ReadTeams(fixture.ctx, db)
			if err != nil {
				return err
			}
			if len(roles) != 1 || roles[0].Model.Provider != mapped || len(teams) != 1 || teams[0].Model.Provider != mapped || teams[0].Members[0] != roles[0].ID {
				return errors.New("dependent references escaped the controller namespace")
			}
			defaults, err := ReadDefaults(fixture.ctx, db)
			if err != nil {
				return err
			}
			if defaults.Config == nil || defaults.Config.Provider != mapped || defaults.Config.Credential == nil || *defaults.Config.Credential != *credential {
				return errors.New("defaults retained a source provider or credential identity")
			}
			sshProfiles, err := ListSsh(fixture.ctx, db)
			if err != nil {
				return err
			}
			databases, err := ListDatabases(fixture.ctx, db)
			if err != nil {
				return err
			}
			if len(sshProfiles) != 1 || len(databases) != 1 || sshProfiles[0].ID == ssh.ID || databases[0].ID == database.ID || databases[0].Connection.Data.(*protocol.DatabaseConnectionSshPayload).Ssh != sshProfiles[0].ID {
				return errors.New("database tunnel references escaped the controller namespace")
			}
			if err := CheckConfigurationEditable(fixture.ctx, db, "providers", id); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				return errors.New("mirror lost its controlling source")
			}
			manifest, err := ReadConfigurationManifest(fixture.ctx, db, fixture.caller, "providers")
			if err != nil || len(manifest.Records) != 1 || manifest.Records[0].ID != provider.ID.String() {
				return errors.New("transport manifest lost the original source identity")
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
	if localIDs["Controller A"] == localIDs["Controller B"] || localIDs["Controller A"] == provider.ID.String() || localIDs["Controller B"] == provider.ID.String() {
		t.Fatal("controller provider identities were not isolated")
	}
	if err := applyFollowFixture(t, storage, second, node, source, "providers", 2, followRecord(t, provider.ID.String(), 2, provider, nil)); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
		t.Fatalf("controller A reached controller B's namespace: %v", err)
	}
	if err := applyFollowFixture(t, storage, first, node, source, "providers", 2); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(local, func(db *sql.Conn) error {
		saved, err := readProviderConfiguration(local, db, provider.ID)
		if err == nil && (saved == nil || saved.Provider.Name != "Host-local provider" || saved.Provider.Source != nil) {
			return errors.New("controller mirror changed host-local configuration")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(second, func(db *sql.Conn) error {
		values, err := providerConfigurations(second, db)
		if err == nil && (len(values) != 1 || !values[0].Listed || values[0].Provider.Name != "Controller B") {
			return errors.New("controller A deletion changed controller B")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationFollowKeepsReplacementRoleIdentity(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.Revision = protocol.NewRoleID(), "reviewer", "Reviewer", 1
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, followRecord(t, role.ID.String(), 1, role, nil)); err != nil {
		t.Fatal(err)
	}
	local := followLocalID(t, storage, ctx, "roles", role.ID.String())
	role.ID, role.Name = protocol.NewRoleID(), "Replacement reviewer"
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 2, followRecord(t, role.ID.String(), 1, role, nil)); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		values, err := ReadRoles(ctx, db)
		if err != nil {
			return err
		}
		if len(values) != 1 || values[0].ID.String() != local || values[0].Name != role.Name {
			return errors.New("stale source cleanup deleted the replacement role")
		}
		if err := CheckConfigurationEditable(ctx, db, "roles", local); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return errors.New("replacement role lost its follow lock")
		}
		manifest, err := ReadConfigurationManifest(ctx, db, source, "roles")
		if err == nil && (len(manifest.Records) != 1 || manifest.Records[0].ID != role.ID.String()) {
			return errors.New("replacement role retained stale transport metadata")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationFollowPreservesLegacyIdentityAcrossUserMerge(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.Revision = protocol.NewRoleID(), "legacy", "Legacy role", 1
	record := followRecord(t, role.ID.String(), 1, role, nil)
	var target protocol.User
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		if err := SetConfigurationSource(ctx, db, node, source, &source, 0); err != nil {
			return err
		}
		if _, _, err := applyConfigurationRecord(configurationApplyContext(ctx), db, node, "roles", record); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO configuration_follow_records(user,category,id,source,revision,digest,state,message,body,materialized) VALUES(?1,'roles',?2,?3,1,?4,'applied','',?5,1)", user.ID.String(), record.ID, source[:], record.Digest, record.Body); err != nil {
			return err
		}
		target, err = CreateUser(ctx, db, "Explicit shared controller group")
		if err != nil {
			return err
		}
		_, err = MergeIdentityUsers(ctx, db, user.ID, target.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	ctx = WithUser(t.Context(), target)
	role.Name, role.Revision = "Updated legacy role", 2
	if err := applyFollowFixture(t, storage, ctx, node, source, "roles", 1, followRecord(t, role.ID.String(), 2, role, nil)); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		values, err := ReadRoles(ctx, db)
		if err != nil {
			return err
		}
		if len(values) != 1 || values[0].ID != role.ID || values[0].Name != role.Name {
			return errors.New("legacy or merged replica identity changed")
		}
		if err := CheckConfigurationEditable(ctx, db, "roles", role.ID.String()); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return errors.New("legacy merge lost the replica lock")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationFollowRejectsConflictingUserMergeBeforeMutation(t *testing.T) {
	t.Parallel()
	storage, node, source, first := followFixture(t)
	secondSource := protocol.NodeID{8}
	var second context.Context
	var firstUser, secondUser protocol.User
	if err := storage.Node.Write(first, func(db *sql.Conn) error {
		var err error
		firstUser, err = CurrentUser(first, db)
		if err != nil {
			return err
		}
		if err := PairLinkPeer(first, db, secondSource, link.DirectionController); err != nil {
			return err
		}
		secondUser, err = CallerUser(first, db, node, secondSource)
		second = WithUser(t.Context(), secondUser)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	provider := providerFixture()
	provider.Revision = 1
	for index, fixture := range []struct {
		caller protocol.NodeID
		ctx    context.Context
	}{{source, first}, {secondSource, second}} {
		key := protocol.NewSecret([]string{"first-controller-key", "second-controller-key"}[index])
		if err := applyFollowFixture(t, storage, fixture.ctx, node, fixture.caller, "providers", 1, followRecord(t, provider.ID.String(), 1, provider, &key)); err != nil {
			t.Fatal(err)
		}
	}
	if err := storage.Node.Write(first, func(db *sql.Conn) error {
		if _, err := MergeIdentityUsers(first, db, firstUser.ID, secondUser.ID); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
			return errors.New("incompatible controller sources were merged")
		}
		// Catch the conflict and retain the transaction so a late rejection cannot
		// hide partial membership/configuration changes behind outer rollback.
		for _, entry := range []struct {
			caller protocol.NodeID
			user   protocol.User
			ctx    context.Context
		}{{source, firstUser, first}, {secondSource, secondUser, second}} {
			current, err := CallerUser(entry.ctx, db, node, entry.caller)
			if err != nil || current.ID != entry.user.ID || current.State != "active" {
				return errors.New("merge conflict changed controller membership")
			}
			providers, err := providerConfigurations(entry.ctx, db)
			if err != nil {
				return err
			}
			if len(providers) != 1 || providers[0].Provider.Owner == nil || *providers[0].Provider.Owner != entry.user.ID || providers[0].Provider.Credential == nil {
				return errors.New("merge conflict changed personal provider ownership")
			}
			status, err := ReadConfigurationFollow(entry.ctx, db)
			if err != nil || status.Source == nil || *status.Source != entry.caller {
				return errors.New("merge conflict discarded a controlling source")
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestConfigurationFollowStaleAssociationCannotChangeAnotherUser(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	provider := providerFixture()
	provider.Name, provider.Revision = "Host-owned provider", 1
	var local context.Context
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		local = WithUser(t.Context(), owner)
		if err := saveProviderConfiguration(local, db, providerConfiguration{ID: provider.ID, Provider: &provider, Listed: true}); err != nil {
			return err
		}
		if err := SetConfigurationSource(ctx, db, node, source, &source, 0); err != nil {
			return err
		}
		record := followRecord(t, provider.ID.String(), 1, provider, nil)
		_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_records(user,category,id,source,revision,digest,state,message,body,materialized) VALUES(?1,'providers',?2,?3,1,?4,'applied','',?5,1)", user.ID.String(), record.ID, source[:], record.Digest, record.Body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	changed := provider
	changed.Name, changed.Revision = "Controller update", 2
	if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 2, followRecord(t, provider.ID.String(), 2, changed, nil)); err != nil {
		t.Fatal(err)
	}
	if err := applyFollowFixture(t, storage, ctx, node, source, "providers", 3); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(local, func(db *sql.Conn) error {
		saved, err := readProviderConfiguration(local, db, provider.ID)
		if err == nil && (saved == nil || saved.Provider == nil || !saved.Listed || saved.Provider.Name != provider.Name || saved.Provider.Revision != provider.Revision) {
			return errors.New("stale replica metadata changed another user's provider")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

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

func TestConfigurationFollowReplacesOnlyReceivingBundledPackage(t *testing.T) {
	t.Parallel()
	storage, node, source, ctx := followFixture(t)
	current := pluginTestInfo("bundled-mirror", 'a')
	version := "0.1.23"
	current.Summary.Version = &version
	var local context.Context
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		local = WithUser(t.Context(), owner)
		if _, err := InstallPlugin(local, db, current, 0, true); err != nil {
			return err
		}
		_, err = InstallPlugin(ctx, db, current, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	incoming := current
	incoming.Summary.Digest, incoming.Summary.Revision, incoming.Summary.Enabled = pluginTestInfo("bundled-mirror", 'b').Summary.Digest, 6, true
	schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","properties":{"label":{"type":"string"}},"additionalProperties":false}`))
	if err != nil {
		t.Fatal(err)
	}
	incoming.Settings = &schema
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		return CacheConfigurationPackage(ctx, db, incoming, protocol.PluginMcpConfiguration{Servers: map[string]protocol.PluginMcpServer{}})
	}); err != nil {
		t.Fatal(err)
	}
	if err := applyFollowFixture(t, storage, ctx, node, source, "plugins", 1, followRecord(t, incoming.Summary.Name, 6, incoming, nil)); err != nil {
		t.Fatal(err)
	}
	settings := pluginFollowSettings{Package: plugins.PackageReference(incoming), Values: map[string]json.RawMessage{"label": json.RawMessage(`"controller"`)}, Allowed: map[string]bool{}}
	if err := applyFollowFixture(t, storage, ctx, node, source, "plugin_settings", 1, followRecord(t, incoming.Summary.Name, 6, settings, nil)); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		remote, err := ReadPlugin(ctx, db, incoming.Summary.Name)
		if err != nil {
			return err
		}
		host, err := ReadPlugin(local, db, current.Summary.Name)
		if err != nil {
			return err
		}
		if remote == nil || host == nil || remote.Summary.Digest != incoming.Summary.Digest || host.Summary.Digest != current.Summary.Digest {
			return errors.New("exact package mirror escaped its receiving user")
		}
		resolved, err := ResolvePluginSettings(ctx, db, plugins.PackageReference(*remote))
		if err != nil || string(resolved.Values["label"]) != `"controller"` {
			return errors.New("source settings did not apply to target-local package revision")
		}
		status, err := ReadConfigurationFollow(ctx, db)
		if err == nil && len(status.Issues) != 0 {
			return errors.New("same-version package mirror left pending settings")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}
