package live

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLegacyPluginNavigationProjection(t *testing.T) {
	const oldSSH = "678994d174ad4bc45be8c3289445fa89c092b348ef57a392b3535f25bfe532cc"
	const lastBundledSSH = "9990c72c27c647a3000cb1385b26550af1e9c5eb2a7cd2823aaed3c21e484a8f"
	const oldFiles = "ac133d0ee39e44e1dfc86fdd42656b57258220180c19c55e193ed0e2d1f7649e"
	thirdParty := strings.Repeat("f", 64)
	for _, row := range []struct {
		label, name, digest string
		registered, visible bool
	}{
		{"legacy navigation without a replacement", "ssh", oldSSH, false, true},
		{"legacy navigation with a replacement", "ssh", oldSSH, true, false},
		{"last bundled navigation with a replacement", "ssh", lastBundledSSH, true, false},
		{"unrelated ssh navigation", "ssh", thirdParty, true, true},
		{"retired files navigation", "files", oldFiles, false, false},
		{"unrelated files navigation", "files", thirdParty, false, true},
		{"another name with the historical digest", "git", oldFiles, false, true},
	} {
		t.Run(row.label, func(t *testing.T) {
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = row.name, row.digest, true
			info.Summary.Compatibility.Kind = "compatible"
			info.Origin = &protocol.PluginOrigin{Kind: "official"}
			ext := protocol.DefaultPluginExtension()
			ext.Desktop = &protocol.PluginDesktopManifest{
				Navigation:        &protocol.PluginDesktopNavigation{Label: "Package navigation"},
				NavigationOptions: protocol.DefaultPluginDesktopNavigationOptions(),
			}
			page := protocol.DefaultPluginDesktopSettings()
			page.Navigation.Label, page.EnabledControl = "Package settings", true
			ext.SettingsPage = &page
			info.Extension = &ext
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
			root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
			root.packages.infos[row.name] = info
			if row.registered {
				root.App = shell.NewLiveWithAssembly(root.App.Data, shell.Assembly{WorkspacePages: []shell.NativePage{{
					Page: "fixture:ssh", Feature: shell.Feature{ID: "fixture-ssh", LegacyPackage: "ssh"}, Render: func(*ui.Context, *shell.App) {},
				}}})
			}
			root.projectPluginViews(nil)
			entry, visible := root.pluginViews.navigation["plugin:"+snapshot.Node.String()+"/"+row.name]
			if visible != row.visible || visible && entry.scope.Package != packageReference(info.Summary) {
				t.Fatalf("navigation projection = %+v (%v), want visible %v", entry.scope, visible, row.visible)
			}
			if entry := root.pluginViews.settings[pluginSettingsSection(root, info)]; entry.scope.Package != packageReference(info.Summary) {
				t.Fatal("navigation suppression changed package settings")
			}
		})
	}
}

func TestLegacyPluginStatusProjection(t *testing.T) {
	const oldGit = "c878628617b254c50d7d493e6dbf7e68a343499e4882cf712d213e65c26478d4"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
	worktree := protocol.WorktreeID{2}
	scope := protocol.PluginContext{Worktree: &worktree}
	statusScope := protocol.PluginUiStatusScope{Worktree: &worktree}
	encoded, err := json.Marshal(statusScope)
	if err != nil {
		t.Fatal(err)
	}
	legacy := protocol.DefaultPluginUiStatusEntry()
	legacy.Package = protocol.PluginReference{Name: "git", Digest: oldGit}
	legacy.Declaration.ID, legacy.Declaration.Slot, legacy.Declaration.Kind = "changes", protocol.PluginUiSlotStatus, "button"
	legacy.Declaration.Label.Label, legacy.Declaration.Intent, legacy.Invoke = "Historical Git", new("changes"), new("invoke")
	legacy.State.ID, legacy.State.Enabled, legacy.State.Visible = "changes", true, true
	unrelated := legacy
	unrelated.Package.Digest, unrelated.Declaration.Label.Label = strings.Repeat("f", 64), "Unrelated Git"
	root.pluginStatus = map[string]*pluginStatusFeed{string(encoded): {
		scope: statusScope, cancel: func() {}, view: pluginStatusView{Connected: true, Entries: []protocol.PluginUiStatusEntry{legacy, unrelated}},
	}}
	registry := plugins.NewRegistry(plugins.RegistryOptions{Node: snapshot.Node, Context: scope})
	t.Cleanup(func() { _ = registry.Close() })
	var comp composer.State
	var projected composer.Context
	root.projectPluginStatus(&comp, &projected, scope, registry)
	if badges := projected.Activity.Badges; len(badges) != 1 || badges[0].ID != pluginStatusIdentity(unrelated) {
		t.Fatalf("status projection = %+v", badges)
	}
	if root.invokePluginStatus(scope, pluginStatusIdentity(legacy)) {
		t.Fatal("a retired status could still be invoked directly")
	}
	legacy.State.Enabled = false
	root.pluginStatus[string(encoded)].view.Entries[0] = legacy
	got := root.tryPluginIntent(scope, registry, "changes", nil)
	if !got.ready || !got.value || got.err != nil {
		t.Fatalf("the retired status intercepted a third-party intent: %+v", got)
	}
}

func TestLegacyProjectLauncherKeepsMixedPublication(t *testing.T) {
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "commands", strings.Repeat("f", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	info.Extension = new(protocol.DefaultPluginExtension())
	info.Extension.Desktop = &protocol.PluginDesktopManifest{
		UiEntry:   new("dev.sailry.platform/ui.js"),
		Renderers: []protocol.PluginDesktopRenderer{{Resource: protocol.PluginDesktopResourceKindTerminal, Create: new("terminal-new")}},
	}
	if err := json.Unmarshal([]byte(`[
		{"id":"terminal-new","slot":"project","kind":"button","label":{"label":"Old launcher"},"handler":"launch","intent":"legacy-launch"},
		{"id":"terminal-menu-new","slot":"project_menu","kind":"button","label":{"label":"Unrelated action"},"handler":"other","intent":"own-action"}
	]`), &info.Extension.Ui); err != nil {
		t.Fatal(err)
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response)})
	root.packages.infos[info.Summary.Name] = info
	root.pluginControlReplacements = shell.Assemble(testassembly.RegisterTerminalFixture).PluginControlReplacements
	scope := protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "project"}}
	registry := root.pluginRegistry(scope)
	root.syncPluginRegistry(nil, root.pluginViews.registries[pluginScopeIdentity(root.controller.Target(), scope)])
	var mounted *pluginMountedView
	for _, current := range root.pluginViews.mounts {
		mounted = current
	}
	if mounted == nil || mounted.failure != nil {
		t.Fatal("the independent action did not mount")
	}
	if err := mounted.hooks.Publish(json.RawMessage(`[{"id":"terminal-new"},{"id":"terminal-menu-new"}]`)); err != nil {
		t.Fatalf("the legacy renderer broke publication of the unrelated action: %v", err)
	}
	if err := registry.SetActivity(mounted.scope.Package, true, false); err != nil {
		t.Fatal(err)
	}
	if _, found := registry.Key(mounted.scope.Package, "terminal-new"); found {
		t.Fatal("the unsupported renderer's launcher stayed addressable")
	}
	if declared, _ := registry.IntentReady("legacy-launch"); declared {
		t.Fatal("the unsupported renderer's launcher stayed invocable")
	}
	if entries := registry.Entries(protocol.PluginUiSlotProjectMenu); len(entries) != 1 || entries[0].Key.ID != "terminal-menu-new" {
		t.Fatalf("unrelated project menu projection = %+v", entries)
	}
}
