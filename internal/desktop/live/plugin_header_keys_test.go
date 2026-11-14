package live

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginHeaderKeepsTitleAndControlsDistinct(t *testing.T) {
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "header-fixture", strings.Repeat("a", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface.Kind = packageReference(info.Summary), "settings"
	section := settings.Section("plugin:header-fixture")
	key := pluginMountKey{scope: pluginScopeIdentity(snapshot.Node, scope), role: "settings", name: info.Summary.Name}
	root.pluginViews.settings[section] = pluginSettingsEntry{section: section, owner: root, info: info, scope: scope}
	ctx, cancel := context.WithCancel(root.ctx)
	host := desktopplugins.NewHost(desktopplugins.HostBinding{Context: ctx, Client: root.controller, Scope: scope})
	components, err := desktopplugins.NativeComponents(host.Native())
	if err != nil {
		t.Fatal(err)
	}
	mounted := &pluginMountedView{key: key, owner: root, info: info, scope: scope, locale: root.packageLocale(),
		generation: 44, ctx: ctx, cancel: cancel, host: host, components: components, mounted: new(desktopplugins.Mounted)}
	root.pluginViews.mounts[key] = mounted
	// This fixture renders native header components directly, without a JS view.
	t.Cleanup(func() { mounted.mounted = nil })
	root.App.Settings.Open(section)
	root.App.Navigate(shell.SettingsPage)
	content := `{"tabs":{"id":"sections","items":[{"id":"overview","label":"Overview"}],"selected":"overview"},"items":[{"label":"Requests","value":"1"}]}`
	props, err := json.Marshal([]any{"header", map[string]string{"content": content}})
	if err != nil {
		t.Fatal(err)
	}
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		for _, component := range components {
			if component.Module == "sailry" && component.Export == "Header" {
				if _, err := component.Render(view.Key("publication"), desktopplugins.NativeElement{Props: props}, nil, nil); err != nil {
					t.Fatal(err)
				}
			}
		}
		ui.Row(view.Key("shared-header")).Children(func() {
			if !root.renderPluginTitle(view) {
				t.Fatal("plugin title was not rendered")
			}
			root.renderPluginHeader(view)
		})
	}, 640, 120)
	for _, generation := range []uint64{44, 44, 45, 45} {
		mounted.generation = generation
		tester.Frame()
		if !tester.HasText("Overview") || !tester.HasText("Requests") || !tester.HasText("1") {
			t.Fatalf("plugin header lost independent content: %v", tester.Texts())
		}
	}
}
