package live

import (
	"context"
	"strings"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginWorkspaceSlots(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "workspace-fixture", strings.Repeat("a", 64), true
		info.Summary.Compatibility.Kind = "compatible"
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
		root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
		synctest.Wait()
		scope := protocol.DefaultPluginContext()
		scope.Package, scope.Surface.Kind = packageReference(info.Summary), "workspace"
		key := pluginMountKey{scope: pluginScopeIdentity(snapshot.Node, scope), role: "navigation", name: info.Summary.Name}
		root.pluginViews.navigation["fixture"] = pluginNavigationEntry{owner: root, info: info, scope: scope}
		calls := map[string]int{}
		makeMount := func(generation uint64, label string) *pluginMountedView {
			ctx, cancel := context.WithCancel(root.ctx)
			mounted := &pluginMountedView{key: key, owner: root, info: info, scope: scope, locale: root.packageLocale(), generation: generation, ctx: ctx, cancel: cancel}
			mounted.navigation = func(view *ui.Context) error {
				calls[label+" navigation"]++
				ui.Button(view.Key("resource"), label+" navigation")
				return nil
			}
			mounted.details = func(view *ui.Context) error {
				calls[label+" details"]++
				ui.Text(view.Key("detail"), label+" details")
				return nil
			}
			mounted.workspaceLayout = func() desktopplugins.WorkspaceLayout {
				return desktopplugins.WorkspaceLayout{ID: "workspace", MinNavigationWidth: 200, MinDetailsWidth: 260, DefaultDetailsWidth: 400}
			}
			return mounted
		}
		mounted := makeMount(1, "First")
		root.pluginViews.mounts[key] = mounted
		root.App.Navigate(shell.PluginPage)
		root.App.Hooks.PluginPage = func(view *ui.Context, _ string) {
			ui.Text(view, "Workspace content")
			root.showPluginDetails(root.pluginViews.mounts[key])
		}
		root.showPluginDetails(mounted)
		tester := ui.NewTesterWith(func(view *ui.Context) {
			clear(calls)
			root.App.View(view)
			for label, count := range calls {
				if count != 1 {
					t.Fatalf("%s rendered %d times in one frame", label, count)
				}
			}
		}, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
		for range 3 {
			tester.Frame()
		}
		if calls["First navigation"] != 1 || calls["First details"] != 1 {
			t.Fatalf("workspace slots were not drawn: %v", calls)
		}
		button, shown := tester.AccessByRole(ui.RoleButton, "First navigation")
		if !shown || button.Bounds.X < shell.RailWidth || button.Bounds.X >= shell.RailWidth+shell.NavWidth {
			t.Fatalf("navigation is outside the application sidebar: %+v", button)
		}
		tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		for range 3 {
			tester.Frame()
		}
		if button, shown = tester.AccessByRole(ui.RoleButton, "First navigation"); !shown || !button.Focused {
			t.Fatal("navigation keys did not retain focus across snapshots")
		}
		// The left column still holds the preceding mount's closure until
		// the page projects its replacement. It must not invoke that closure.
		mounted.cancel()
		root.pluginViews.mounts[key] = makeMount(2, "Second")
		tester.Frame()
		if calls["First navigation"] != 0 || calls["First details"] != 0 {
			t.Fatalf("retired mount was drawn: %v", calls)
		}
		tester.Frame()
		if calls["Second navigation"] != 1 || calls["Second details"] != 1 {
			t.Fatalf("replacement slots were not projected on the next frame: %v", calls)
		}
		root.App.Navigate(shell.ConversationPage)
		tester.Frame()
		if calls["Second navigation"] != 0 || calls["Second details"] != 0 {
			t.Fatal("the conversation invoked a retained plugin slot")
		}
	})
}
