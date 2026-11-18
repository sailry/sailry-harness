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
	betaOnly := plugin("beta-only", "f", protocol.PluginScopeHost)
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
	root.App.Settings.ManagedNode = alpha.controller.Target().String()
	root.App.Settings.Open(section)
	root.App.Navigate(shell.SettingsPage)
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
		settings.Page(view, root.App.Settings, nil, settings.Look{})
	}, 760, 460)
	tester.SetLocale("en")
	renderFrom := func(owner *Runtime, source *pluginViewTestNode, expected protocol.PluginInfo, text string) {
		t.Helper()
		if root.App.Settings.ManagedNode != owner.controller.Target().String() {
			if err := tester.Click(i18n.In("en", "project_host")); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if err := tester.Click("Beta"); err != nil {
				t.Fatal(err)
			}
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
		if !slices.ContainsFunc(root.App.Settings.PluginPages(), func(page settings.PluginPage) bool {
			return page.Section == pluginSettingsSection(root, localGame)
		}) {
			t.Fatal("the controller's desktop-only page disappeared")
		}

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
	if _, exists := root.pluginViews.settings["plugin:beta-only"]; exists {
		t.Fatal("Alpha catalog displayed Beta's installation")
	}
	renderFrom(beta, betaSource, betaVersion, "Beta search view")
	if _, exists := root.pluginViews.settings["plugin:alpha-only"]; exists {
		t.Fatal("Beta catalog displayed Alpha's installation")
	}
	if len(localSource.reads) != 0 {
		t.Fatal("the local client was asked for a remote view")
	}

	root.App.Settings.ManagedNode = alpha.controller.Target().String()
	root.projectPluginSettings(nil)
	root.App.Settings.Open("plugin:alpha-only")
	root.App.Settings.ManagedNode = beta.controller.Target().String()
	tester.Frame()
	if entry := root.pluginViews.settings["plugin:alpha-only"]; entry.owner != nil || entry.status != "plugins_not_installed_on_host" ||
		root.App.Settings.Section != "plugin:alpha-only" || !tester.HasText("Not installed on this host") {
		t.Fatalf("switching away from an installed-only page did not preserve its unavailable view: %+v, %v", entry, tester.Texts())
	}
	root.App.Settings.ManagedNode = (protocol.NodeID{4}).String()
	tester.Frame()
	if entry := root.pluginViews.settings["plugin:alpha-only"]; entry.owner != nil || entry.status != "plugins_view_offline" ||
		!tester.HasText("Host disconnected") {
		t.Fatalf("missing host did not show the offline view: %+v, %v", entry, tester.Texts())
	}
}
