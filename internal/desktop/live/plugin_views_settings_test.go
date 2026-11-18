package live

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginSettingsViewRecovery(t *testing.T) {
	const entryPath = "dev.sailry.platform/desktop/settings.js"
	const viewText = "Recovered custom settings"
	for _, tc := range []struct {
		name  string
		leave func(*shell.App)
	}{
		{"re-enter section", func(app *shell.App) { app.Settings.Open(settings.General) }},
		{"reopen Settings", func(app *shell.App) { app.Navigate(shell.ConversationPage) }},
		{"refresh from another Node", nil},
	} {
		t.Run(tc.name, func(t *testing.T) {
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest = "settings-fixture", strings.Repeat("f", 64)
			info.Summary.Compatibility.Kind = "compatible"
			info.Extension = new(protocol.DefaultPluginExtension())
			info.Extension.Scope = protocol.PluginScopeHost
			info.Extension.SettingsPage = new(protocol.DefaultPluginDesktopSettings())
			info.Extension.SettingsPage.Navigation.Label = "Fixture settings"
			info.Extension.SettingsPage.Entry = new(entryPath)
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
			source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1), reads: make(chan protocol.Request, 4)}
			root := pluginViewFixture(t, source)
			root.packages.infos[info.Summary.Name] = info

			otherSnapshot := protocol.DefaultSnapshot()
			otherSnapshot.Node = protocol.NodeID{2}
			other := pluginViewFixture(t, &pluginViewTestNode{snapshot: otherSnapshot})
			other.parent, other.App, other.busy = root, root.App, map[string]bool{}
			root.bindings = map[string]*Runtime{otherSnapshot.Node.String(): other}
			root.App.Sidebar.Selected = "draft/first"
			root.App.Composer("draft/first").Host = otherSnapshot.Node.String()
			if root.selectedRuntime() != other {
				t.Fatal("Refresh must route to a different Node from the settings view")
			}

			section := pluginSettingsSection(root, info)
			root.App.Settings.Open(section)
			root.App.Navigate(shell.SettingsPage)
			reference := packageReference(info.Summary)
			key := pluginMountKey{scope: pluginScopeKey{node: snapshot.Node, surface: "settings"}, role: "settings", name: reference.Name}
			tester := ui.NewTesterWith(func(view *ui.Context) {
				theme.Apply(view, "", theme.Light)
				root.mu.Lock()
				queued := root.queue
				root.queue = nil
				root.mu.Unlock()
				for _, apply := range queued {
					apply(view)
				}
				root.resumeReadsOnReconnect()
				other.resumeReadsOnReconnect()
				root.pluginViewsFrame(view)
				if root.App.Page == shell.SettingsPage {
					ui.Column(view).Fill().Children(func() { settings.Page(view, root.App.Settings, root.App.Data, settings.Look{}) })
				}
				kit.ToastHost(view)
				root.pluginViewsAfterFrame()
			}, 960, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			tester.SetLocale("en")
			refresh := func() {
				root.Refresh()
				other.workers.Wait()
				tester.Frame()
			}
			completeRead := func(response link.Response) {
				t.Helper()
				source.bundle <- response
				root.workers.Wait()
				select {
				case request := <-source.reads:
					payload, ok := detail[protocol.CommandReadPluginViewPayload](request.Command.Data)
					if request.Target != snapshot.Node || request.Command.Kind != "read_plugin_view" || !ok ||
						payload.Package != reference || payload.Surface.Kind != "settings" {
						t.Fatalf("view read changed scope: %+v", request)
					}
				default:
					t.Fatal("expected one immutable settings view read")
				}
				tester.Frame()
			}
			framesWithoutRead := func() {
				t.Helper()
				generation := root.pluginViews.generation
				for range 3 {
					tester.Frame()
				}
				if root.pluginViews.generation != generation || len(source.reads) != 0 {
					t.Fatal("ordinary frames started another view read")
				}
			}

			tester.Frame()
			completeRead(link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture view temporarily unavailable"}})
			failed := root.pluginViews.mounts[key]
			if failed == nil || faultCode(failed.failure) != protocol.ErrorCodeUnavailable || failed.ctx.Err() == nil ||
				!tester.HasText(i18n.In("en", "plugins_view_load_failed")) || tester.HasText(i18n.In("en", "chat_retry")) {
				t.Fatalf("expected a failed view without retry controls: %v", tester.Texts())
			}
			other.readsConnected = false
			framesWithoutRead()
			if root.pluginViews.mounts[key] != failed {
				t.Fatal("an unrelated reconnect or ordinary frame changed the failed view")
			}

			if tc.leave != nil {
				tc.leave(root.App)
				framesWithoutRead()
				root.App.Settings.Open(section)
				root.App.Navigate(shell.SettingsPage)
				tester.Frame()
			} else {
				refresh()
			}
			completeRead(link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
				Package: reference, Entry: entryPath,
				Files: map[string]string{entryPath: `import {View,div} from "gpui-kit";export default class extends View{render(){return div().child("` + viewText + `");}}`},
			}}})
			recovered := root.pluginViews.mounts[key]
			if recovered == nil || recovered == failed || recovered.locale != failed.locale || recovered.mounted == nil || !tester.HasText(viewText) {
				t.Fatalf("settings view did not recover in the same scope: %v", tester.Texts())
			}

			refresh()
			framesWithoutRead()
			root.App.Navigate(shell.ConversationPage)
			framesWithoutRead()
			root.App.Navigate(shell.SettingsPage)
			framesWithoutRead()
			if root.pluginViews.mounts[key] != recovered || !tester.HasText(viewText) {
				t.Fatal("refresh or re-entry changed the healthy settings mount")
			}
		})
	}
}
