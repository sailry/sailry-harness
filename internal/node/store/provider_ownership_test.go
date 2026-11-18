package store

import (
	"database/sql"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderAndDefaultsAreOwnedByEachNode(t *testing.T) {
	t.Parallel()
	storage, node, _, ctx := controllerCacheFixture(t)
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
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestRemoveProviderWithHistoricalOrigin(t *testing.T) {
	t.Parallel()
	storage, node, controller, ctx := controllerCacheFixture(t)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		id := protocol.NewProviderID()
		now := time.Now()
		session, turn, err := createAdmittedLocalProviderTurn(ctx, db, storage, node, controller, id, now)
		if err != nil {
			return err
		}
		saved, err := currentProvider(ctx, db, id, nil)
		if err != nil {
			return err
		}
		origin := protocol.ConfigurationOrigin{Node: controller, Revision: 7}
		saved.Provider.Source = &origin
		if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
			return err
		}
		legacy := *saved.Provider
		legacy.Revision = origin.Revision
		legacy.Credential = &protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
		if err := seedHistoricalConfiguration(ctx, db, "providers", id.String(), legacy.Revision, legacy); err != nil {
			return err
		}
		runtime := protocol.RuntimeCredential{
			Controller: controller, Kind: "provider", ID: legacy.Credential.ID.String(), Generation: legacy.Credential.ID.String(),
			Revision: 1, State: "ready", Provider: &id, Authentication: protocol.AuthenticationApiKey, Access: protocol.NewSecret("historical-source-key"),
		}
		if err := seedHistoricalRuntimeCredentials(ctx, db, runtime); err != nil {
			return err
		}
		if err := installHistoricalRemoteCapture(ctx, db, session, turn.ID, controller, legacy); err != nil {
			return err
		}
		projected, _, err := ReadProviderSnapshot(ctx, db, node)
		if err != nil {
			return err
		}
		if len(projected) != 1 || projected[0].ID != id || projected[0].Shared || projected[0].Source == nil || *projected[0].Source != origin {
			return errors.New("historical provenance changed the host provider's scope")
		}
		remove := protocol.NewRequest(node, protocol.Command{Kind: "remove_provider", Data: &protocol.CommandRemoveProviderPayload{Provider: id, ExpectedRevision: projected[0].Revision}})
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		if _, _, err := ExecuteProviderCommand(WithUser(ctx, local), db, node, remove, nil); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
			return fmt.Errorf("host administrator could remove another user's provider: %v", err)
		}
		stale := remove
		stale.Command.Data = &protocol.CommandRemoveProviderPayload{Provider: id, ExpectedRevision: origin.Revision}
		if _, _, err := ExecuteProviderCommand(ctx, db, node, stale, nil); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
			return fmt.Errorf("source revision was accepted as the host revision: %v", err)
		}
		output, events, err := ExecuteProviderCommand(ctx, db, node, remove, nil)
		if err != nil {
			return err
		}
		if output.Kind != "providers" || len(output.Data.([]protocol.ConversationProvider)) != 0 || len(events) == 0 || events[0].Kind != "provider_removed" {
			return errors.New("removal did not unlist the host provider")
		}
		source, err := readCachedConfiguration[protocol.ConversationProvider](ctx, db, "providers", id.String())
		if err != nil || source == nil || source.Revision != origin.Revision || source.Credential == nil || *source.Credential != *legacy.Credential {
			return fmt.Errorf("host removal changed the preserved source: %v", err)
		}
		if key, err := resolveCapturedProviderKey(ctx, db, node, *saved.Provider.Credential, *saved.Provider); err != nil || key == nil || key.Expose() != "local-capture-fixture-key" {
			return fmt.Errorf("host removal invalidated its captured key: %v", err)
		}
		invocation, _, err := ClaimNext(ctx, db, now)
		if err != nil || invocation == nil || invocation.Turn.ID != turn.ID || invocation.AuthorizationFailure != nil || invocation.Secret == nil || invocation.Secret.Expose() != runtime.Access.Expose() {
			return fmt.Errorf("host removal changed the admitted historical turn: %v", err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestHistoricalRuntimeCredentialsExcludedFromNewProviderChoices(t *testing.T) {
	t.Parallel()
	storage, node, controller, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := providerFixture()
		provider.Revision = 1
		provider.Credential = &protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
		if err := seedHistoricalConfiguration(ctx, db, "providers", provider.ID.String(), provider.Revision, provider); err != nil {
			return err
		}
		legacyDefaults := protocol.DefaultDefaults()
		legacyConfig := protocol.DefaultSessionConfig()
		legacyConfig.Provider, legacyConfig.Model, legacyConfig.Credential = provider.ID, provider.DefaultModel, provider.Credential
		legacyConfig.Mode, legacyConfig.Permission = protocol.WorkModeCode, protocol.PermissionProject
		legacyDefaults.Config = &legacyConfig
		if err := seedHistoricalConfiguration(ctx, db, "defaults", "default", 1, legacyDefaults); err != nil {
			return err
		}
		runtime := protocol.RuntimeCredential{Controller: controller, Kind: "provider", ID: provider.Credential.ID.String(),
			Generation: provider.Credential.ID.String(), Revision: 1, State: "ready", Provider: &provider.ID,
			Authentication: protocol.AuthenticationApiKey, Access: protocol.NewSecret("controller-key")}
		if err := seedHistoricalRuntimeCredentials(ctx, db, runtime); err != nil {
			return err
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

func TestNodeOwnedRolesAndTeamsExcludeLegacyFollowedModels(t *testing.T) {
	t.Parallel()
	storage, node, _, ctx := controllerCacheFixture(t)
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		foreign := &protocol.RoleModel{Provider: protocol.NewProviderID(), Model: "controller-only"}
		role := protocol.DefaultRoleProfile()
		role.ID, role.Revision, role.Key, role.Name, role.Model = protocol.NewRoleID(), 1, "reviewer", "Reviewer", foreign
		team := protocol.DefaultTeamProfile()
		team.ID, team.Revision, team.Key, team.Name, team.Model = protocol.NewTeamID(), 1, "review", "Review", foreign
		team.Members = []protocol.RoleID{role.ID}
		if err := seedHistoricalConfiguration(ctx, db, "roles", role.ID.String(), 1, role); err != nil {
			return err
		}
		if err := seedHistoricalConfiguration(ctx, db, "teams", team.ID.String(), 1, team); err != nil {
			return err
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
		if len(roles.Profiles) != 2 || roles.Profiles[0].Key != "explore" || roles.Profiles[1].Key != "research" || len(roles.Providers) != 0 {
			return errors.New("a new session did not use the Node's default roster")
		}
		model := &protocol.RoleModel{Provider: local.ID, Model: local.DefaultModel}
		role.Model, team.Model = model, model
		if _, err := putRole(ctx, db, role, 0); err != nil {
			return err
		}
		if _, err := putTeam(ctx, db, team, 0); err != nil {
			return err
		}
		selection := &protocol.TeamSelection{ID: team.ID}
		roles, err = CaptureSessionRoles(ctx, db, node, protocol.ConversationExecutionTeam, selection, nil)
		if err != nil || len(roles.Providers) != 1 || roles.Providers[0].ID != local.ID {
			return fmt.Errorf("the Node-local team could not select its own role model: %v", err)
		}
		effective, err := configureSessionTeam(ctx, db, config, selection)
		if err != nil || effective.Provider != local.ID {
			return fmt.Errorf("the Node-local team did not resolve its own model: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
