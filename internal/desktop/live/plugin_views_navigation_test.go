package live

import (
	"context"
	"encoding/json"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHasPluginSettingsContent(t *testing.T) {
	info := protocol.DefaultPluginInfo()
	if hasPluginSettingsContent(info) {
		t.Fatal("plugin without an extension has a settings page")
	}

	extension := protocol.DefaultPluginExtension()
	page := protocol.DefaultPluginDesktopSettings()
	extension.SettingsPage = &page
	info.Extension = &extension
	if hasPluginSettingsContent(info) {
		t.Fatal("navigation-only settings page was included")
	}

	page.EnabledControl = true
	if !hasPluginSettingsContent(info) {
		t.Fatal("enabled control was omitted")
	}
	page.EnabledControl = false
	entry := "dev.sailry.platform/desktop/settings.js"
	page.Entry = &entry
	if !hasPluginSettingsContent(info) {
		t.Fatal("custom settings page was omitted")
	}
	page.Entry = nil
	info.Settings = new(protocol.PluginSettingsSchema)
	if !hasPluginSettingsContent(info) {
		t.Fatal("schema settings page was omitted")
	}
}

func TestPluginSettingsUseSelectedHost(t *testing.T) {
	const path = "dev.sailry.platform/desktop/settings.js"
	plugin := func(name, digest string, scope protocol.PluginScope) protocol.PluginInfo {
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = name, strings.Repeat(digest, 64), true
		info.Summary.Compatibility.Kind = "compatible"
		ext := protocol.DefaultPluginExtension()
		ext.Scope = scope
		ext.Actions = []protocol.PluginAction{protocol.PluginActionReadStorage}
		page := protocol.DefaultPluginDesktopSettings()
		page.Navigation.Label, page.Entry = name, new(path)
		page.Placement.Group, page.Placement.Order = protocol.PluginDesktopSettingsGroupApp, 10
		ext.SettingsPage, info.Extension = &page, &ext
		return info
	}
	fixture := func(node protocol.NodeID, plugins ...protocol.PluginInfo) (*Runtime, *pluginViewTestNode) {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		for _, item := range plugins {
			snapshot.Plugins = append(snapshot.Plugins, item.Summary)
		}
		source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1), reads: make(chan protocol.Request, 4)}
		owner := pluginViewFixture(t, source)
		for _, item := range plugins {
			owner.packages.infos[item.Summary.Name] = item
		}
		return owner, source
	}
	bundle := func(info protocol.PluginInfo, text string) link.Response {
		return link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
			Package: packageReference(info.Summary), Entry: path,
			Files: map[string]string{path: `import {View,div} from "gpui-kit";export default class extends View{render(){return div().child("` + text + `");}}`},
		}}}
	}
	localVersion := plugin("search", "a", protocol.PluginScopeHost)
	alphaVersion := plugin("search", "b", protocol.PluginScopeHost)
	betaVersion := plugin("search", "c", protocol.PluginScopeHost)
	localGame := plugin("game", "d", protocol.PluginScopeDesktop)
	alphaOnly := plugin("alpha-only", "e", protocol.PluginScopeHost)
	alphaOnly.Extension.SettingsPage.Entry = nil
	alphaOnly.Extension.SettingsPage.EnabledControl = true
	betaOnly := plugin("beta-only", "f", protocol.PluginScopeHost)
	betaOnly.Summary.Enabled = false
	root, localSource := fixture(protocol.NodeID{1}, localVersion, localGame)
	alpha, alphaSource := fixture(protocol.NodeID{2}, alphaVersion, alphaOnly)
	beta, betaSource := fixture(protocol.NodeID{3}, betaVersion, betaOnly)
	alpha.parent, alpha.App = root, root.App
	beta.parent, beta.App = root, root.App
	root.bindings = map[string]*Runtime{alpha.controller.Target().String(): alpha, beta.controller.Target().String(): beta}
	localSource.bundle <- bundle(localVersion, "Local search view")
	alphaSource.bundle <- bundle(alphaVersion, "Alpha search view")
	betaSource.bundle <- bundle(betaVersion, "Beta search view")

	const section settings.Section = "plugin:search"
	root.App.Settings.ProviderActions.Hosts = func() []settings.ProviderHost {
		return []settings.ProviderHost{
			{Node: root.controller.Target().String(), Local: true, Connected: true},
			{Node: alpha.controller.Target().String(), Name: "Alpha", Connected: true},
			{Node: beta.controller.Target().String(), Name: "Beta", Connected: true},
		}
	}
	if !root.openPluginSettings(alpha, alphaVersion) || root.App.Page != shell.SettingsPage || root.App.Settings.Section != section {
		t.Fatalf("opening the plugin settings selected page %v and section %q", root.App.Page, root.App.Settings.Section)
	}
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		root.mu.Lock()
		queued := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queued {
			apply(view)
		}
		root.pluginViewsFrame(view)
		ui.Row(view).Fill().Children(func() {
			ui.Column(view.Key("test-settings-nav")).Width(216).FillHeight().Children(func() {
				settings.Nav(view, root.App.Settings)
			})
			ui.Column(view.Key("test-settings-page")).Grow(1).MinWidth(0).FillHeight().Children(func() {
				settings.Page(view, root.App.Settings, nil, settings.Look{})
			})
		})
	}, 976, 460)
	tester.SetLocale("en")
	selectHost := func(name string) {
		t.Helper()
		if err := tester.Click(i18n.In("en", "project_host")); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		if err := tester.Click(name); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
	}
	checkNavigation := func(owner *Runtime, info protocol.PluginInfo, want bool) {
		t.Helper()
		entry := pluginSettingsSection(owner, info)
		inSearch := slices.ContainsFunc(root.App.Settings.PluginPages(), func(page settings.PluginPage) bool { return page.Section == entry })
		_, inSidebar := tester.AccessByRole(ui.RoleButton, info.Extension.SettingsPage.Navigation.Label)
		if inSearch != want || inSidebar != want {
			t.Fatalf("%s in settings search = %t and sidebar = %t, want %t", entry, inSearch, inSidebar, want)
		}
	}
	renderFrom := func(owner *Runtime, source *pluginViewTestNode, expected protocol.PluginInfo, text string) {
		t.Helper()
		if root.App.Settings.ManagedNode != owner.controller.Target().String() {
			selectHost("Beta")
		}
		tester.Frame()
		select {
		case request := <-source.reads:
			payload, ok := detail[protocol.CommandReadPluginViewPayload](request.Command.Data)
			if request.Command.Kind != "read_plugin_view" || !ok || payload.Package != packageReference(expected.Summary) {
				t.Fatalf("bundle read used the wrong installation: %+v", request.Command)
			}
		case <-time.After(2 * time.Second):
			t.Fatal("selected host did not receive a view read")
		}
		owner.workers.Wait()
		tester.Frame()
		if !tester.HasText(text) || tester.HasText("Local search view") || root.App.Settings.Section != section {
			t.Fatalf("active settings view did not follow its owner: %v", tester.Texts())
		}
		checkNavigation(root, localGame, true)

		entry := root.pluginViews.settings[section]
		key := pluginMountKey{scope: pluginScopeIdentity(owner.controller.Target(), entry.scope), role: "settings", name: expected.Summary.Name}
		modules, err := root.pluginViews.mounts[key].host.Modules()
		if err != nil {
			t.Fatal(err)
		}
		sdk := modules[slices.IndexFunc(modules, func(module desktopplugins.HostModule) bool { return module.Name == "sailry/sdk" })]
		ref := packageReference(expected.Summary)
		value := json.RawMessage(`"` + text + `"`)
		cfg := protocol.DefaultPluginSettingsState()
		cfg.Package, cfg.Values["owner"] = ref, value
		kv := protocol.PluginStorageEntry{Key: "owner", Value: value, Present: true, Revision: 1}
		for _, operation := range []struct {
			function string
			command  string
			answer   protocol.Output
			args     []json.RawMessage
		}{
			{"readSettings", "read_plugin_settings", protocol.Output{Kind: "plugin_settings", Data: cfg}, nil},
			{"getValue", "read_plugin_value", protocol.Output{Kind: "plugin_value", Data: kv}, []json.RawMessage{json.RawMessage(`"owner"`)}},
		} {
			source.bundle <- link.Response{Output: operation.answer}
			method := sdk.Exports[slices.IndexFunc(sdk.Exports, func(export desktopplugins.HostExport) bool { return export.Name == operation.function })]
			ctx, cancel := context.WithTimeout(t.Context(), 2*time.Second)
			body, err := method.Async(ctx, operation.args)
			cancel()
			if err != nil || !strings.Contains(string(body), text) {
				t.Fatalf("%s did not read its owner's value: %s, %v", operation.function, body, err)
			}
			request := <-source.reads
			if request.Command.Kind != operation.command || request.Target != owner.controller.Target() || request.Plugin == nil || request.Plugin.Package != ref {
				t.Fatalf("%s did not use its mounted package scope: %+v", operation.function, request)
			}
		}
	}
	renderFrom(alpha, alphaSource, alphaVersion, "Alpha search view")
	checkNavigation(alpha, alphaOnly, true)
	checkNavigation(beta, betaOnly, false)
	renderFrom(beta, betaSource, betaVersion, "Beta search view")
	checkNavigation(alpha, alphaOnly, false)
	checkNavigation(beta, betaOnly, true)
	if len(localSource.reads) != 0 {
		t.Fatal("the local client was asked for a remote view")
	}

	root.App.Settings.Open(settings.General)
	root.App.Settings.ManagedNode = alpha.controller.Target().String()
	tester.Frame()
	if err := tester.Click("alpha-only"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	selectHost("Beta")
	if entry := root.pluginViews.settings["plugin:alpha-only"]; entry.owner != nil || entry.status != "plugins_not_installed_on_host" ||
		root.App.Settings.Section != "plugin:alpha-only" || !tester.HasText("Not installed on this host") {
		t.Fatalf("switching away from an installed-only page did not preserve its unavailable view: %+v, %v", entry, tester.Texts())
	}
	checkNavigation(alpha, alphaOnly, false)
	checkNavigation(beta, betaOnly, true)
	root.App.Settings.ManagedNode = (protocol.NodeID{4}).String()
	tester.Frame()
	if entry := root.pluginViews.settings["plugin:alpha-only"]; entry.owner != nil || entry.status != "plugins_view_offline" ||
		!tester.HasText("Host disconnected") {
		t.Fatalf("missing host did not show the offline view: %+v, %v", entry, tester.Texts())
	}
	checkNavigation(alpha, alphaOnly, false)
	checkNavigation(beta, betaOnly, false)
	checkNavigation(root, localGame, true)
	selectHost("Alpha")
	checkNavigation(alpha, alphaOnly, true)
	if entry := root.pluginViews.settings["plugin:alpha-only"]; entry.owner != alpha || entry.status != "" || !tester.HasText(i18n.In("en", "plugins_enabled")) {
		t.Fatalf("switching back did not restore the installed page: %+v, %v", entry, tester.Texts())
	}
}
