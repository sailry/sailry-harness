package store

import (
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderAndDefaultsAreOwnedByEachNode(t *testing.T) {
	t.Parallel()
	storage, node, controller, ctx := controllerCacheFixture(t)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		provider := providerFixture()
		key := protocol.NewSecret("target-local-key")
		output, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{
			Provider: provider, ExpectedRevision: 0, Secret: &key,
		}}}, nil)
		if err != nil {
			return err
		}
		saved := output.Data.(protocol.ConversationProvider)
		if saved.Credential == nil || saved.Credential.Node != node || saved.Owner == nil || *saved.Owner != user.ID {
			return errors.New("paired user provider did not receive a Node-local credential")
		}
		values, err := providerList(ctx, db)
		if err != nil || len(values) != 1 || values[0].ID != saved.ID {
			return errors.New("paired user could not read its Node-local provider")
		}
		defaults := protocol.DefaultDefaults()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Credential = saved.ID, saved.DefaultModel, saved.Credential
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		defaults.Config = &config
		if _, err := SaveDefaults(ctx, db, node, defaults); err != nil {
			return err
		}
		read, err := ReadDefaults(ctx, db)
		if err != nil || read.Config == nil || read.Config.Provider != saved.ID || read.Config.Credential == nil || *read.Config.Credential != *saved.Credential {
			return errors.New("paired user defaults did not retain its local provider")
		}
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		if _, err := ExportConfigurationManifest(WithUser(ctx, local), db, node, controller, "providers", true, nil, nil); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			return errors.New("provider configuration was accepted for follow export")
		}
		if _, err := ExportConfigurationManifest(WithUser(ctx, local), db, node, controller, "defaults", true, nil, nil); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			return errors.New("model defaults were accepted for follow export")
		}
		if values, err := exportRuntimeCredentials(ctx, db, node, map[protocol.CredentialID]runtimeCapture{saved.Credential.ID: {}}); err != nil || len(values) != 1 {
			return errors.New("runtime credential export for existing captures was removed")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestFollowKeepsCapturedRuntimeCredentialsOutOfNewProviderChoices(t *testing.T) {
	t.Parallel()
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := providerFixture()
		provider.Revision = 1
		provider.Credential = &protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
		manifest := protocol.ConfigurationManifest{Source: controller, Category: "providers", Revision: 1,
			Records: []protocol.ConfigurationFollowRecord{followRecord(t, provider.ID.String(), provider.Revision, provider, nil)}}
		if _, err := ApplyConfigurationManifest(ctx, db, node, controller, manifest); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			return errors.New("follow installed a controller-owned provider")
		}
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		legacyProvider := followRecord(t, provider.ID.String(), provider.Revision, provider, nil)
		if err := cacheConfigurationRecord(ctx, db, user, "providers", legacyProvider); err != nil {
			return err
		}
		legacyDefaults := protocol.DefaultDefaults()
		legacyConfig := protocol.DefaultSessionConfig()
		legacyConfig.Provider, legacyConfig.Model, legacyConfig.Credential = provider.ID, provider.DefaultModel, provider.Credential
		legacyConfig.Mode, legacyConfig.Permission = protocol.WorkModeCode, protocol.PermissionProject
		legacyDefaults.Config = &legacyConfig
		if err := cacheConfigurationRecord(ctx, db, user, "defaults", followRecord(t, "default", 1, legacyDefaults, nil)); err != nil {
			return err
		}
		runtime := protocol.RuntimeCredential{Controller: controller, Kind: "provider", ID: provider.Credential.ID.String(),
			Generation: provider.Credential.ID.String(), Revision: 1, State: "ready", Provider: &provider.ID,
			Authentication: protocol.AuthenticationApiKey, Access: protocol.NewSecret("controller-key")}
		if _, err := applyConfigurationCache(ctx, db, node, controller, protocol.ConfigurationManifest{Source: controller, Category: "runtime_credentials", Revision: 1,
			Records: []protocol.ConfigurationFollowRecord{followRecord(t, runtime.CacheID(), runtime.Revision, runtime, nil)}}); err != nil {
			return err
		}
		var count int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM configuration_cache WHERE user=?1 AND category='runtime_credentials' AND id LIKE 'provider:%'", user.ID.String()).Scan(&count); err != nil {
			return err
		}
		if count != 1 {
			return errors.New("follow did not retain the captured runtime credential")
		}
		values, err := providerList(ctx, db)
		if err != nil {
			return err
		}
		if len(values) != 0 {
			return errors.New("runtime credential cache exposed a provider choice")
		}
		defaults, err := ReadDefaults(ctx, db)
		if err != nil || defaults.Config != nil && defaults.Config.Provider == provider.ID {
			return fmt.Errorf("legacy model default became an executable choice: %+v: %v", defaults, err)
		}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Credential = provider.ID, provider.DefaultModel, provider.Credential
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		if err := validateConfig(ctx, db, node, config); !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
			return fmt.Errorf("legacy cached provider configured a new session: %v", err)
		}
		if access, err := ReadRuntimeProviderCredential(ctx, db, node, provider, *provider.Credential); err != nil || access == nil || access.Expose() != "controller-key" {
			return fmt.Errorf("captured runtime credential was not readable: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestFollowedRoleAndTeamModelsDoNotOverrideExecutionNode(t *testing.T) {
	t.Parallel()
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		foreign := &protocol.RoleModel{Provider: protocol.NewProviderID(), Model: "controller-only"}
		role := protocol.DefaultRoleProfile()
		role.ID, role.Revision, role.Key, role.Name, role.Model = protocol.NewRoleID(), 1, "reviewer", "Reviewer", foreign
		team := protocol.DefaultTeamProfile()
		team.ID, team.Revision, team.Key, team.Name, team.Model = protocol.NewTeamID(), 1, "review", "Review", foreign
		team.Members = []protocol.RoleID{role.ID}
		for _, manifest := range []protocol.ConfigurationManifest{
			{Source: controller, Category: "roles", Revision: 1, Records: []protocol.ConfigurationFollowRecord{followRecord(t, role.ID.String(), 1, role, nil)}},
			{Source: controller, Category: "teams", Revision: 1, Records: []protocol.ConfigurationFollowRecord{followRecord(t, team.ID.String(), 1, team, nil)}},
		} {
			if _, err := applyConfigurationCache(ctx, db, node, controller, manifest); err != nil {
				return err
			}
		}
		provider := providerFixture()
		saved, _, err := ExecuteProviderCommand(ctx, db, node, protocol.Request{Command: protocol.Command{
			Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider},
		}}, nil)
		if err != nil {
			return err
		}
		local := saved.Data.(protocol.ConversationProvider)
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model = local.ID, local.DefaultModel
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		created, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &config})
		if err != nil {
			return err
		}
		roles := created.Data.(protocol.Session).Roles
		if len(roles.Profiles) != 1 || roles.Profiles[0].Model != nil || len(roles.Providers) != 0 {
			return errors.New("the followed role carried its controller model into a new session")
		}
		selection := &protocol.TeamSelection{ID: team.ID}
		models := map[protocol.RoleID]protocol.RoleModel{role.ID: {Provider: local.ID, Model: local.DefaultModel}}
		roles, err = CaptureSessionRoles(ctx, db, node, protocol.ConversationExecutionTeam, selection, models)
		if err != nil || len(roles.Providers) != 1 || roles.Providers[0].ID != local.ID {
			return fmt.Errorf("the followed team could not select an explicit Node-local role model: %v", err)
		}
		effective, err := configureSessionTeam(ctx, db, config, selection)
		if err != nil || effective.Provider != local.ID {
			return fmt.Errorf("the followed team changed the conversation's Node-local model: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
