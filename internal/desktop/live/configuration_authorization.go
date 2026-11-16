package live

import (
	"context"
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// An explicit follow authorization owns only its dialogs. The controller's
// configuration model and the monitored device remain independent.
type configurationAuthorization struct {
	owner   *Runtime
	model   *settings.Model
	source  *settings.ConfigurationFollowEditor
	cancel  context.CancelFunc
	loading bool
}

func (r *Runtime) authorizationSettings() *settings.Model {
	if scope := r.localRuntime().configurationAuthorization; scope != nil && scope.owner == r {
		return scope.model
	}
	return r.App.Settings.Model
}

func (r *Runtime) authorizationView(attempt *liveAuthorization, provider bool) *settings.Authorization {
	if attempt.view == nil {
		if provider {
			attempt.view = r.authorizationSettings().ProviderAuthorization()
		} else {
			attempt.view = &r.authorizationSettings().ConnectorActions.Authorization
		}
	}
	return attempt.view
}

func (r *Runtime) beginConfigurationAuthorization(owner *Runtime, source *settings.ConfigurationFollowEditor, issue settings.ConfigurationFollowIssue) {
	root := r.localRuntime()
	if root.configurationAuthorization != nil {
		return
	}
	owner.initProviders()
	if owner.packages == nil {
		owner.packages = newPackageState()
	}
	ctx, cancel := context.WithCancel(owner.ctx)
	scope := &configurationAuthorization{owner: owner, model: settings.NewLive().Model, source: source, cancel: cancel, loading: true}
	root.configurationAuthorization = scope
	source.Loading = true
	m := scope.model
	m.ProviderActions = settings.ProviderActions{
		SignIn: owner.beginProviderLogin, SubmitLogin: owner.submitProviderLogin, CancelLogin: owner.cancelProviderLogin,
	}
	m.ConnectorActions = &settings.ConnectorActions{
		Save: owner.saveConnector, InspectCLI: owner.inspectConnectorCLI,
		Submit: owner.submitConnectorLogin, Cancel: owner.cancelConnectorLogin,
		Retry: func() {
			if attempt := owner.authorizations.connector; attempt != nil {
				owner.saveConnector(attempt.connector, nil)
			}
		},
		Help: func(id, name string) string {
			for _, field := range owner.authorizations.types[id].Declaration.Fields {
				if field.Name == name && field.Help != nil {
					return *field.Help
				}
			}
			return ""
		},
	}
	m.PackageActions = &settings.PackageActions{SaveSettings: owner.savePackageSettings}
	root.async(func() {
		var listing protocol.ConversationPresetsListing
		var changes protocol.PluginConnectorChanges
		var info protocol.PluginInfo
		var values protocol.PluginSettingsState
		var models protocol.PluginModelsCatalog
		var err error
		switch issue.Category {
		case "providers":
			var out protocol.Output
			out, err = owner.controller.Do(ctx, protocol.Command{Kind: "list_channel_presets", Data: protocol.CommandListChannelPresetsPayload{}})
			var ok bool
			listing, ok = detail[protocol.ConversationPresetsListing](out.Data)
			if err == nil && (!ok || out.Kind != "channel_presets") {
				err = errors.New("unexpected provider presets response")
			}
		case "connections":
			stream, subscribeErr := owner.controller.Subscribe(ctx, protocol.Topic{Kind: "connections"})
			err = subscribeErr
			if err == nil {
				var update protocol.Update
				update, err = stream.Next(ctx)
				_ = stream.Close()
				var ok bool
				changes, ok = detail[protocol.PluginConnectorChanges](update.Data)
				if err == nil && (!ok || update.Kind != "connections_changed") {
					err = errors.New("unexpected connection authorization response")
				}
			}
		case "plugin_settings":
			var out protocol.Output
			out, err = owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: issue.ID}})
			var ok bool
			info, ok = detail[protocol.PluginInfo](out.Data)
			if err == nil && (!ok || out.Kind != "plugin" || info.Summary.Name != issue.ID || info.Settings == nil) {
				err = errors.New("plugin authorization settings are unavailable")
			}
			if err == nil {
				ref := packageReference(info.Summary)
				out, err = owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin_settings", Data: protocol.CommandReadPluginSettingsPayload{Package: ref}})
				values, ok = detail[protocol.PluginSettingsState](out.Data)
				if err == nil && (!ok || out.Kind != "plugin_settings" || values.Package != ref) {
					err = errors.New("unexpected plugin authorization settings response")
				}
				needsModels := false
				for _, field := range info.Settings.Properties {
					needsModels = needsModels || field.Model || field.Effort != nil
				}
				if err == nil && needsModels {
					out, err = owner.controller.Do(ctx, protocol.Command{Kind: "list_plugin_models"})
					models, ok = detail[protocol.PluginModelsCatalog](out.Data)
					if err == nil && (!ok || out.Kind != "plugin_models") {
						err = errors.New("unexpected plugin authorization models response")
					}
				}
			}
		default:
			err = errors.New("unsupported configuration authorization category")
		}
		root.enqueue(func(c *ui.Context) {
			if root.configurationAuthorization != scope {
				return
			}
			source.Loading = false
			if root.App.Settings.FollowEditor != source {
				root.closeConfigurationAuthorization(scope)
				return
			}
			scope.loading = false
			fail := func(problem error) {
				root.closeConfigurationAuthorization(scope)
				root.reportKeys(c, problem, "configuration_follow_failed", "configuration_follow_conflict", "configuration_follow_unknown")
			}
			if err != nil {
				fail(err)
				return
			}
			switch issue.Category {
			case "providers":
				for _, entry := range listing.Entries {
					key := entry.Source.Kind + ":" + entry.Preset.ID
					if entry.Package != nil {
						key += ":" + entry.Package.Name
					}
					owner.providerPresets[key] = entry
				}
				if _, ok := owner.loginProvider(issue.ID); !ok {
					fail(errors.New("provider authorization is unavailable"))
					return
				}
				owner.beginProviderLogin(issue.ID)
			case "connections":
				for _, kind := range changes.Types {
					owner.authorizations.types[connectorTypeID(kind.Package, kind.Declaration.ID)] = kind
				}
				key := ""
				for _, connection := range changes.Connections {
					id := connectorTypeID(connection.Package, connection.Connector)
					owner.authorizations.connections[id] = connection
					if connection.ID.String() == issue.ID || id == issue.ID {
						key = id
					}
				}
				if _, ok := owner.authorizations.types[key]; key == "" || !ok {
					fail(errors.New("connection authorization is unavailable"))
					return
				}
				owner.projectConnectors()
				m.AuthorizeFollowedConnection(key)
			case "plugin_settings":
				owner.packages.infos[issue.ID], owner.packages.settings[issue.ID] = info, values
				m.OpenPackageConfiguration(issue.ID, packageView(info, owner.packageLocale()).Name,
					packageSettingsFields(info.Settings, values, owner.packageLocale(), models), packageSettingsTabs(info.Settings, owner.packageLocale())...)
			}
			if root.App.Settings.FollowEditor == source {
				root.App.Settings.FollowEditor = nil
			}
		})
	})
}

func (r *Runtime) configurationAuthorizationDialog(c *ui.Context) {
	root := r.localRuntime()
	scope := root.configurationAuthorization
	if scope == nil {
		return
	}
	if scope.loading {
		if root.App.Settings.FollowEditor != scope.source {
			root.closeConfigurationAuthorization(scope)
		}
		return
	}
	if !settings.ExplicitAuthorizationDialogs(c.Key("configuration-authorization"), scope.model) {
		root.closeConfigurationAuthorization(scope)
	}
}

func (r *Runtime) closeConfigurationAuthorization(scope *configurationAuthorization) {
	root := r.localRuntime()
	if root.configurationAuthorization != scope {
		return
	}
	state := scope.owner.authorizations
	if attempt := state.provider; attempt != nil && attempt.view == scope.model.ProviderAuthorization() {
		scope.owner.cancelProviderLogin()
		attempt.cancel()
		state.provider = nil
	}
	if attempt := state.connector; attempt != nil && attempt.view == &scope.model.ConnectorActions.Authorization {
		scope.owner.cancelConnectorLogin()
		attempt.cancel()
		state.connector = nil
	}
	scope.cancel()
	scope.source.Loading = false
	root.configurationAuthorization = nil
}
