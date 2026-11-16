package live

import (
	"context"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type configurationAuthorizationTransport struct {
	conversationTestTransport
	connections protocol.PluginConnectorChanges
}

func (source *configurationAuthorizationTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "connections" {
		return source.conversationTestTransport.Subscribe(ctx, topic)
	}
	stream := &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
	stream.updates <- protocol.Update{Kind: "connections_changed", Data: source.connections}
	return stream, nil
}

func TestConfigurationAuthorizationKeepsGlobalSettingsLocal(t *testing.T) {
	for _, category := range []string{"providers", "connections", "plugin_settings"} {
		t.Run(category, func(t *testing.T) {
			localSnapshot, remoteSnapshot := protocol.DefaultSnapshot(), protocol.DefaultSnapshot()
			localSnapshot.Node, remoteSnapshot.Node = protocol.NodeID{1}, protocol.NodeID{2}
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Name, provider.Revision, provider.Authentication = protocol.ProviderID{7}, "Remote account", 5, protocol.AuthenticationClaudeOAuth
			remoteSnapshot.Providers = []protocol.ConversationProvider{provider}
			localSource := &conversationTestTransport{snapshot: localSnapshot}
			remoteSource := &configurationAuthorizationTransport{conversationTestTransport: conversationTestTransport{snapshot: remoteSnapshot}}
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Revision, info.Summary.SettingsRevision = "remote-package", 3, 4
			info.Settings = &protocol.PluginSettingsSchema{Properties: map[string]protocol.PluginSettingsField{"account": {Kind: protocol.PluginSettingsKindString}}}
			reference := packageReference(info.Summary)
			connection := protocol.DefaultPluginConnectorConnection()
			connection.ID, connection.Package, connection.Connector = protocol.NewRequestID(), reference, "account"
			connection.Method, connection.Revision, connection.State = protocol.PluginConnectorKindOauth2, 8, protocol.PluginConnectorStateNotConnected
			kind := protocol.DefaultPluginConnectorType()
			kind.Package, kind.Enabled = reference, true
			kind.Declaration.ID, kind.Declaration.Display.Label = "account", "Remote connection"
			kind.Declaration.Auth = []protocol.PluginConnectorAuth{{Kind: "oauth2", Data: protocol.PluginConnectorAuthOauth2Payload{Flow: protocol.PluginConnectorFlowDeviceCode}}}
			remoteSource.connections = protocol.PluginConnectorChanges{Types: []protocol.PluginConnectorType{kind}, Connections: []protocol.PluginConnectorConnection{connection}}
			attemptID := protocol.NewRequestID()
			remoteSource.handle = func(request protocol.Request) (link.Response, bool) {
				output := func(kind string, data any) (link.Response, bool) {
					return link.Response{Output: protocol.Output{Kind: kind, Data: data}}, true
				}
				switch request.Command.Kind {
				case "configuration_follow":
					return output("configuration_follow", protocol.ConfigurationFollowStatus{})
				case "list_channel_presets":
					return output("channel_presets", protocol.DefaultConversationPresetsListing())
				case "begin_provider_login":
					return output("provider_login", protocol.ConversationLoginAttempt{ID: attemptID})
				case "start_connector_login":
					return output("connector_login", protocol.PluginConnectorAttempt{ID: attemptID})
				case "read_plugin":
					return output("plugin", info)
				case "read_plugin_settings", "save_plugin_settings":
					return output("plugin_settings", protocol.PluginSettingsState{Package: reference})
				case "complete_provider_login", "cancel_provider_login":
					return output("provider_login", protocol.ConversationLoginAttempt{ID: attemptID, Provider: provider.ID})
				case "submit_connector_input", "cancel_connector_login":
					return output("connector_login", protocol.PluginConnectorAttempt{ID: attemptID, Connection: connection.ID, Package: reference})
				}
				return link.Response{}, false
			}
			localClient, remoteClient := client.New(localSource), client.New(remoteSource)
			if err := localClient.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			if err := remoteClient.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer localClient.Close()
			defer remoteClient.Close()
			ctx, cancel := context.WithCancel(t.Context())
			app := shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
			root := &Runtime{ctx: ctx, controller: localClient, App: app, packages: newPackageState()}
			remote := &Runtime{ctx: ctx, parent: root, controller: remoteClient, App: app, packages: newPackageState(), Preferences: preferences.Open(t.TempDir() + "/preferences.json"), busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
			root.bindings = map[string]*Runtime{remoteSnapshot.Node.String(): remote}
			defer func() { cancel(); root.workers.Wait(); remote.workers.Wait(); remote.observers.Wait() }()
			app.Host = "monitored-device"
			app.Settings.Channels = []viewmodel.Channel{{ID: "local-account", Name: "Local account"}}
			app.Settings.Services = []viewmodel.ConnectorService{{ID: "local-connection", Name: "Local connection"}}
			app.Settings.Compaction.Draft = 87
			localView := app.Settings.ProviderAuthorization()
			*localView = settings.Authorization{Title: "Local authorization", Pasted: "retained"}
			root.initConfigurationFollowActions()
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				root.mu.Lock()
				queue := root.queue
				root.queue = nil
				root.mu.Unlock()
				for _, complete := range queue {
					complete(c)
				}
				kit.ToastHost(c)
			}, 900, 700)
			settle := func() {
				for range 4 {
					root.workers.Wait()
					remote.workers.Wait()
					tester.Frame()
				}
			}
			app.Settings.FollowActions.Open(viewmodel.Device{ID: remoteSnapshot.Node.String()})
			settle()
			id := provider.ID.String()
			if category == "connections" {
				// Follow issues carry the persistent connection UUID, whereas
				// the editor's service key is package/connector.
				id = connection.ID.String()
			} else if category == "plugin_settings" {
				id = info.Summary.Name
			}
			app.Settings.FollowActions.Authorize(app.Settings.FollowEditor, settings.ConfigurationFollowIssue{Category: category, ID: id, Authorize: true})
			settle()
			scope := root.configurationAuthorization
			if scope == nil || scope.loading || scope.owner != remote || scope.model == app.Settings.Model || app.Settings.FollowEditor != nil {
				t.Fatal("explicit authorization did not open an isolated captured model")
			}
			dialog := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				root.configurationAuthorizationDialog(c)
			}, 900, 700)
			dialog.SetLocale("en")
			dialog.Frame()
			if root.configurationAuthorization != scope {
				t.Fatal("captured authorization dialog closed before user action")
			}
			if category == "providers" {
				if !dialog.HasText(provider.Name) || !scope.model.ProviderAuthorization().Open || remote.authorizations.provider == nil || remote.authorizations.provider.view == localView {
					t.Fatal("remote provider login replaced the local authorization")
				}
				scope.model.ProviderActions.SubmitLogin("fixture-callback")
				settle()
				scope.model.ProviderActions.CancelLogin()
				settle()
			} else if category == "connections" {
				if !dialog.HasText(kind.Declaration.Display.Label) {
					t.Fatal("remote connection UUID did not open its authorization form")
				}
				draft := settings.ConnectorDraft{Type: connectorTypeID(reference, connection.Connector), Service: connectorTypeID(reference, connection.Connector), Method: "connector_oauth", ExpectedRevision: connection.Revision}
				scope.model.ConnectorActions.Save(draft, nil)
				settle()
				if remote.authorizations.connector == nil || remote.authorizations.connector.view != &scope.model.ConnectorActions.Authorization {
					t.Fatal("connector login did not retain its isolated authorization")
				}
				scope.model.ConnectorActions.Submit("fixture-callback")
				settle()
				scope.model.ConnectorActions.Cancel()
				settle()
			} else {
				saved := false
				scope.model.PackageActions.SaveSettings(id, []viewmodel.Setting{{Name: "account", Value: "fixture"}}, nil, func(ok bool) { saved = ok })
				settle()
				if !saved {
					t.Fatal("explicit remote package settings did not save")
				}
			}
			if app.Host != "monitored-device" || root.shownSettings() != root || root.settingsOwner() != root || root.packageOwner() != root || app.Settings.Compaction.Draft != 87 || localView.Title != "Local authorization" || localView.Pasted != "retained" {
				t.Fatal("remote authorization rebound ordinary controller configuration")
			}
			if len(app.Settings.Channels) != 1 || app.Settings.Channels[0].ID != "local-account" || len(app.Settings.Services) != 1 || app.Settings.Services[0].ID != "local-connection" {
				t.Fatal("remote authorization replaced local configuration lists")
			}
			if category == "providers" {
				dialog.Frame()
			} else {
				root.closeConfigurationAuthorization(scope)
			}
			if root.configurationAuthorization != nil || remote.authorizationSettings() != app.Settings.Model {
				t.Fatal("closed authorization retained remote dialog ownership")
			}
			localSource.mu.Lock()
			defer localSource.mu.Unlock()
			if len(localSource.requests) != 0 {
				t.Fatal("explicit remote authorization was sent to the local Node")
			}
			remoteSource.mu.Lock()
			defer remoteSource.mu.Unlock()
			var submitted, cancelled bool
			for _, request := range remoteSource.requests {
				if request.Target != remoteSnapshot.Node {
					t.Fatal("authorization request lost its captured Node")
				}
				if category == "providers" {
					submitted = submitted || request.Command.Kind == "complete_provider_login"
					cancelled = cancelled || request.Command.Kind == "cancel_provider_login"
				} else if category == "connections" {
					submitted = submitted || request.Command.Kind == "submit_connector_input"
					cancelled = cancelled || request.Command.Kind == "cancel_connector_login"
				}
			}
			if category != "plugin_settings" && (!submitted || !cancelled) {
				t.Fatal("remote callback or cancellation did not reach the captured Node")
			}
		})
	}
}
