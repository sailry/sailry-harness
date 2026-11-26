package live

import (
	"context"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	browserdesktop "github.com/sailry/sailry-harness/modules/browser/desktop"
	computerdesktop "github.com/sailry/sailry-harness/modules/computer/desktop"
	connectordesktop "github.com/sailry/sailry-harness/modules/connectors/desktop"
	memorydesktop "github.com/sailry/sailry-harness/modules/memory/desktop"
	teamsdesktop "github.com/sailry/sailry-harness/modules/teams/desktop"
)

type settingsPageTransport struct {
	*settingsTestTransport
	opened, pythonOpened   atomic.Int32
	streamed, pythonStream chan *conversationTestSubscription
}

func (transport *settingsPageTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	var stream *conversationTestSubscription
	switch topic.Kind {
	case "connections":
		transport.opened.Add(1)
		stream = &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
		stream.updates <- protocol.Update{Kind: "connections", Data: protocol.PluginConnectorChanges{}}
		transport.streamed <- stream
	case "python":
		transport.pythonOpened.Add(1)
		stream = &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
		stream.updates <- protocol.Update{Kind: "python", Data: protocol.UpdatePythonPayload{Node: transport.snapshot.Node}}
		transport.pythonStream <- stream
	default:
		return transport.settingsTestTransport.Subscribe(ctx, topic)
	}
	return stream, nil
}

func (transport *settingsPageTransport) count(command string) int {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	n := 0
	for _, request := range transport.requests {
		if request.Command.Kind == command {
			n++
		}
	}
	return n
}

func settingsPageFixture(t *testing.T, node protocol.NodeID, admin bool) (*client.Client, *settingsPageTransport, protocol.Snapshot) {
	t.Helper()
	provider := providerRouteFixture()
	provider.Enabled = true
	provider.Authentication = protocol.AuthenticationChatGptOAuth
	provider.Credential = &protocol.CredentialRef{Node: node, ID: protocol.CredentialID{1}}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Providers = node, []protocol.ConversationProvider{provider}
	snapshot.User = &protocol.User{ID: protocol.UserID{node[0]}, Administrator: admin}
	snapshot.Roles = []protocol.RoleProfile{{Name: "Settings fixture", Revision: 1}}
	snapshot.Teams = []protocol.TeamProfile{{Name: "Settings team", Revision: 1}}
	fresh := uint64(time.Now().UnixMilli())
	snapshot.ModelCatalog.UpdatedAtMs, snapshot.ModelCatalog.Scores.UpdatedAtMs = &fresh, &fresh
	source := &conversationTestTransport{snapshot: snapshot}
	source.handle = func(request protocol.Request) (link.Response, bool) {
		output := func(kind string, data any) (link.Response, bool) {
			return link.Response{Output: protocol.Output{Kind: kind, Data: data}}, true
		}
		switch request.Command.Kind {
		case "list_channel_presets":
			return output("channel_presets", protocol.DefaultConversationPresetsListing())
		case "list_credentials":
			return output("credentials", []protocol.Credential{{ID: provider.Credential.ID, Provider: provider.ID}})
		case "read_compaction_settings":
			return output("compaction_settings", protocol.ConversationCompactionSettings{Revision: 1, Percent: 80})
		case "read_team_settings":
			return output("team_settings", protocol.TeamSettings{Revision: 1, ChildConcurrency: 4})
		case "read_media_settings":
			return output("media_settings", protocol.MediaSettings{Revision: 1})
		case "list_media_models":
			return output("media_models", []protocol.MediaCandidate{})
		case "read_search_settings":
			return output("search_settings", protocol.SearchSettings{Revision: 1})
		case "list_plugin_models":
			return output("plugin_models", protocol.PluginModelsCatalog{})
		case "read_memory_catalog":
			return output("memory_catalog", protocol.MemoryCatalog{})
		case "read_computer_settings":
			return output("computer_settings", protocol.ComputerSettings{Revision: uint64(node[0]), Available: true})
		case "read_external_browser_settings":
			return output("external_browser_settings", protocol.ExternalBrowserSettings{Revision: 1, Available: true})
		case "read_hooks":
			stored := protocol.DefaultHooksSettings()
			stored.Revision = 1
			return output("hook_settings", protocol.HooksState{Settings: stored})
		case "read_python_settings":
			return output("python_settings", protocol.PythonInfo{Version: "3.12.13", Settings: protocol.PythonSettings{Revision: 1}})
		}
		return link.Response{}, false
	}
	transport := &settingsPageTransport{settingsTestTransport: &settingsTestTransport{conversationTestTransport: source},
		streamed: make(chan *conversationTestSubscription, 4), pythonStream: make(chan *conversationTestSubscription, 4)}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	return controller, transport, snapshot
}

func mountSettingsPageFixture(t *testing.T, controller *client.Client, assembly shell.Assembly) (*Runtime, func()) {
	t.Helper()
	r, err := NewWithAssembly(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"), assembly)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = r.Close() })
	window := ui.NewTesterWith(r.drain, 1, 1, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	return r, func() {
		for range 5 {
			r.workers.Wait()
			for _, peer := range r.bindings {
				peer.workers.Wait()
			}
			window.Frame()
		}
	}
}

func settingsPageStream(t *testing.T, topic string, opened <-chan *conversationTestSubscription) *conversationTestSubscription {
	t.Helper()
	select {
	case stream := <-opened:
		return stream
	case <-time.After(3 * time.Second):
		t.Fatalf("the selected Node did not open its %s subscription", topic)
		return nil
	}
}

func TestSettingsPageContributions(t *testing.T) {
	pages := []struct {
		section settings.Section
		reads   []string
		shown   func(*settings.State) bool
	}{
		{settings.Providers, []string{"list_channel_presets", "list_credentials", "read_compaction_settings"}, func(m *settings.State) bool { return len(m.Channels) == 1 && m.Compaction.Revision == 1 }},
		{settings.Teams, []string{"read_team_settings"}, func(m *settings.State) bool {
			return len(m.Roles) == 1 && len(m.Teams) == 1 && m.Concurrency.Revision == 1 &&
				len(m.TeamChannels) == 1 && len(m.TeamChannels[0].Models) != 0
		}},
		{settings.Connectors, nil, func(m *settings.State) bool { return m.ConnectorActions != nil }},
		{settings.Media, []string{"read_media_settings", "list_media_models"}, func(m *settings.State) bool { return m.Media.Revision == 1 }},
		{settings.Search, []string{"read_search_settings", "list_plugin_models"}, func(m *settings.State) bool { return m.Search.Loaded && m.SearchActions != nil }},
		{settings.Memory, []string{"read_memory_catalog"}, func(m *settings.State) bool { return m.MemorySettings.Loaded && m.MemoryActions != nil }},
		{settings.Computer, []string{"read_computer_settings"}, func(m *settings.State) bool { return m.Computer.Loaded && m.ComputerActions != nil }},
		{settings.Browser, []string{"read_external_browser_settings"}, func(m *settings.State) bool { return m.Browser.ExternalLoaded && m.BrowserActions != nil }},
		{settings.Hooks, []string{"read_hooks"}, func(m *settings.State) bool { return m.Hooks.Revision == 1 && m.SaveHooks != nil }},
		{settings.Runtime, []string{"read_python_settings"}, func(m *settings.State) bool {
			return m.PythonVersion == "3.12.13" && m.PackageActions.SavePython != nil
		}},
	}
	browser := shell.Assemble(browserdesktop.Register)
	settingsOnly := shell.Assembly{SettingsPages: browser.SettingsPages, Locales: browser.Locales}
	browser.SettingsPages = nil
	for _, fixture := range []struct {
		name     string
		assembly shell.Assembly
		want     func(settings.Section) bool
	}{
		{"empty", shell.Assembly{}, func(settings.Section) bool { return false }},
		{"Harness", desktop.Harness(), func(settings.Section) bool { return true }},
		{"combo without providers", shell.Assemble(connectordesktop.Register, memorydesktop.Register), func(section settings.Section) bool {
			return section == settings.Connectors || section == settings.Memory
		}},
		{"Teams without provider settings", shell.Assemble(teamsdesktop.Register), func(section settings.Section) bool { return section == settings.Teams }},
		{"Browser panel only", browser, func(settings.Section) bool { return false }},
		{"Browser settings only", settingsOnly, func(section settings.Section) bool { return section == settings.Browser }},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			controller, transport, snapshot := settingsPageFixture(t, protocol.NodeID{1}, true)
			r, settle := mountSettingsPageFixture(t, controller, fixture.assembly)
			m := r.App.Settings
			r.App.Navigate(shell.SettingsPage)
			settle()
			for _, page := range pages {
				for _, read := range page.reads {
					if got := transport.count(read); got != 0 {
						t.Fatalf("hidden %s sent %s %d times", page.section, read, got)
					}
				}
			}
			if r.authorizations != nil || transport.opened.Load() != 0 || transport.pythonOpened.Load() != 0 || m.ManagedHosts == nil {
				t.Fatal("the window started optional provider or connector work before opening a page")
			}
			if (m.ProviderActions.Hosts != nil) != fixture.want(settings.Providers) {
				t.Fatal("the shared host selector bound an absent provider editor")
			}
			comp := composer.New("")
			r.defaultComposer(comp)
			if comp.Model != modelKey(snapshot.Providers[0].ID, snapshot.Providers[0].DefaultModel) || transport.count("read_model_scores") == 0 {
				t.Fatalf("chat model directory without provider management: default %q, models %+v", comp.Model, r.App.Data.Models)
			}
			for _, page := range pages {
				want := fixture.want(page.section)
				if m.HasSection(page.section) != want {
					t.Fatalf("availability of %s did not match the assembly", page.section)
				}
				// A stale route with the old section key must not wake an omitted page.
				m.Section = page.section
				settle()
				if want != page.shown(m) {
					t.Fatalf("the %s projection did not match the assembly", page.section)
				}
				for _, read := range page.reads {
					if got := transport.count(read); got > 1 || (got != 0) != want {
						t.Fatalf("%s sent %d times; want %v", read, got, want)
					}
				}
				var stream *conversationTestSubscription
				if want {
					switch page.section {
					case settings.Connectors:
						stream = settingsPageStream(t, "connections", transport.streamed)
					case settings.Runtime:
						stream = settingsPageStream(t, "python", transport.pythonStream)
					}
				}
				m.Open(settings.General)
				settle()
				if page.shown(m) || r.settingsPage(page.section) != nil {
					t.Fatalf("hiding %s retained its visible binding", page.section)
				}
				if stream != nil {
					select {
					case <-stream.closed:
					case <-time.After(3 * time.Second):
						t.Fatalf("hiding %s left its subscription alive", page.section)
					}
				}
			}
			if got := transport.opened.Load(); got > 1 || (got != 0) != fixture.want(settings.Connectors) {
				t.Fatalf("connections subscriptions = %d", got)
			}
			if got := transport.pythonOpened.Load(); got > 1 || (got != 0) != fixture.want(settings.Runtime) {
				t.Fatalf("Python subscriptions = %d", got)
			}
			for _, section := range []settings.Section{settings.OCR, settings.Decision, settings.Knowledge} {
				if !m.HasSection(section) {
					m.Section = section
					settle()
				}
			}
			if transport.count("read_local_model_settings") != 0 || transport.count("list_knowledge_collections") != 0 {
				t.Fatal("a stale route started an omitted local model or knowledge settings read")
			}
			if fixture.name == "Browser settings only" {
				m.ManagedNode = "offline-host"
				m.Open(settings.Browser)
				settle()
				if r.App.Panel.Offers(panel.Browser) || m.Browser.ExternalStatus != "connections_offline" ||
					m.BrowserActions == nil || m.BrowserActions.ScanChrome == nil || m.BrowserActions.SaveExternal != nil {
					t.Fatal("Browser settings lost window-local controls when the Node or panel was absent")
				}
			}
		})
	}
}

func TestSettingsPageTargetPermissionAndRestore(t *testing.T) {
	local, localSource, _ := settingsPageFixture(t, protocol.NodeID{1}, true)
	remote, remoteSource, remoteSnapshot := settingsPageFixture(t, protocol.NodeID{2}, true)
	const contributed = settings.Section("settings_fixture_lifecycle")
	var bound, opened, closed, released, followed int
	var target settings.PageTarget
	assembly := shell.Assemble(computerdesktop.Register, func(a *shell.Assembly) {
		a.SettingsPages = append(a.SettingsPages, settings.NativePage{Section: contributed, BindWindow: func(any) settings.WindowBinding {
			bound++
			return settings.WindowBinding{Close: func() { released++ }, Mount: func(next settings.PageTarget) settings.PageBinding {
				opened++
				target = next
				return settings.PageBinding{Follow: func() { followed++ }, Close: func() { closed++ }}
			}}
		}})
	})
	r, settle := mountSettingsPageFixture(t, local, assembly)
	peer := r.newBinding(remote)
	r.bindings = map[string]*Runtime{remote.Target().String(): peer}
	m := r.App.Settings
	r.App.Navigate(shell.SettingsPage)
	m.Open(contributed)
	settle()
	if bound != 1 || opened != 1 || followed == 0 || !target.Current() || target.Node != local.Target().String() {
		t.Fatal("a fresh contribution was not bound once and mounted on the local Node")
	}
	original := target
	m.ManagedNode = remote.Target().String()
	if original.Current() {
		t.Fatal("the old target was still current before the next frame")
	}
	settle()
	if opened != 2 || closed != 1 || original.Context.Err() == nil || !target.Current() || target.Node != remote.Target().String() {
		t.Fatal("the contribution was not moved to the selected Node")
	}
	m.Open(settings.Computer)
	settle()
	if stopped := target.Context.Err(); stopped == nil || m.ComputerActions == nil || m.Computer.Enabled.Revision != 2 || localSource.count("read_computer_settings") != 0 {
		t.Fatalf("wrong Node or late release: %v, %+v", stopped, m.Computer)
	}
	previous := m.ComputerActions
	role := func(admin bool) {
		t.Helper()
		remoteSnapshot.Cursor++
		user := *remoteSnapshot.User
		user.Administrator = admin
		remoteSnapshot.User = &user
		remoteSource.report(remoteSnapshot)
		deadline := time.Now().Add(3 * time.Second)
		for remote.Snapshot().User.Administrator != admin {
			if time.Now().After(deadline) {
				t.Fatal("the fixture's authority change did not reach the Client")
			}
			time.Sleep(time.Millisecond)
		}
	}
	role(false)
	previous.Save()
	settle()
	if m.ComputerActions != nil || m.Computer.Status != "permission_denied" || m.Computer.Enabled.Revision != 0 || remoteSource.count("save_computer_settings") != 0 {
		t.Fatalf("revocation exposed settings or admitted a stale action: %+v", m.Computer)
	}
	role(true)
	settle()
	if m.ComputerActions == nil || m.ComputerActions == previous || m.Computer.Enabled.Revision != 2 || remoteSource.count("read_computer_settings") != 2 {
		t.Fatalf("restoring authority did not bind and reread the selected Node: %+v", m.Computer)
	}
	if err := r.Close(); err != nil {
		t.Fatal(err)
	}
	if released != 1 || closed != 2 || m.ComputerActions != nil {
		t.Fatal("closing the window did not release its mounted and hidden contributions")
	}
}
