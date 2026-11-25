package shell

import (
	"context"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestNativeWorkspaceSlots(t *testing.T) {
	const route Page = "example.com/ssh"
	const key = "plugin:fixture/ssh/ssh-workspace"
	const details = "Fixture Assistant"
	active, closes := false, 0
	page := NativePage{
		Page: route, Feature: Feature{ID: string(route), Label: "Fixture SSH", Icon: icons.Terminal, DefaultPinned: true},
		Render: func(c *ui.Context, _ *App) { ui.Button(c, "Fixture PTY") },
		Workspace: func(*App) WorkspaceSlots {
			slots := WorkspaceSlots{
				PreferenceKey: key, MinNavigationWidth: 280, MinDetailsWidth: 320,
				DetailsLabel: details, Title: func(c *ui.Context) { ui.Button(c, "Fixture tab") },
				Header: func(c *ui.Context) { ui.Text(c, "Fixture actions") },
				Close:  func(*ui.Context) { closes++ },
			}
			if active {
				slots.Navigation = func(c *ui.Context) { ui.Text(c, "Fixture files") }
				slots.Details = func(c *ui.Context) { ui.Text(c, "Fixture session") }
				slots.DetailsHeader = func(c *ui.Context) { ui.Text(c, "Fixture session heading") }
			}
			return slots
		},
	}
	base := New(preview.Load())
	base.Navigate(route)
	if base.NativePageEnabled(route) || base.Page != ConversationPage {
		t.Fatal("a native workspace is reachable without its module")
	}
	a := New(preview.Load(), page)
	prefs := a.Prefs()
	prefs.PluginWorkspaces = map[string]PluginWorkspacePrefs{key: {Width: 360, Folded: true}}
	a.SetPrefs(prefs)
	a.Navigate(route)
	tester := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tester.SetLocale("en")
	still(tester)
	if a.HasNavigation() || a.hasDetails() || !tester.HasText("Fixture actions") || !tester.HasText("Fixture tab") {
		t.Fatal("the landing page did not show the header without resource columns")
	}
	active = true
	still(tester)
	files, shown := tester.Find("Fixture files")
	main, mainShown := tester.Find("Fixture PTY")
	if !shown || !mainShown || !a.NavShown() || files.X < RailWidth || main.X < RailWidth+280 || sessionsShown(tester) {
		t.Fatalf("resource navigation %v and terminal %v did not share the shell columns", files, main)
	}
	if tester.HasText("Fixture session") || a.pluginWorkspacePrefs().Width != 360 {
		t.Fatal("the legacy workspace width and folded state were not restored")
	}
	clickButton(t, tester, details)
	settle(tester)
	panel, panelShown := tester.Find("Fixture session")
	heading, headingShown := tester.Find("Fixture session heading")
	if !panelShown || !headingShown || panel.X < 1280-360 || heading.Y >= HeaderHeight || panel.Y < HeaderHeight {
		t.Fatalf("details %v and header %v were not drawn in their shell slots", panel, heading)
	}
	tester.Key(ui.Cmd, ui.KeyB)
	settle(tester)
	if a.NavShown() || tester.HasText("Fixture files") {
		t.Fatal("the shell navigation shortcut did not fold the files")
	}
	clickButton(t, tester, i18n.In("en", "sidebar_toggle"))
	settle(tester)
	if !a.NavShown() || !tester.HasText("Fixture files") {
		t.Fatal("the shell navigation toggle did not restore the files")
	}
	clickButton(t, tester, "Fixture PTY")
	tester.Key(ui.Cmd, ui.KeyW)
	tester.Frame()
	if closes != 1 || a.Page != route {
		t.Fatal("Close Tab in the terminal did not stay in the native workspace")
	}
	clickButton(t, tester, "Fixture tab")
	a.Do(CloseFocused.ID)
	tester.Frame()
	if closes != 2 || a.Page != route {
		t.Fatal("Close Tab from the title did not invoke the native workspace action")
	}
	a.Navigate(ConversationPage)
	still(tester)
	if tester.HasText("Fixture files") || tester.HasText("Fixture session") || tester.HasText("Fixture tab") {
		t.Fatal("leaving the native route kept its slots")
	}
}

func TestNativeWindowBindingBelongsToOneWindow(t *testing.T) {
	const route Page = "example.com/bound-workspace"
	type windowID struct{}
	type instance struct {
		visible func() bool
		synced  int
		closed  int
	}
	instances := map[string]*instance{}
	page := NativePage{
		Page: route, Feature: Feature{ID: string(route), Label: "Fixture workspace", Icon: icons.Terminal},
		BindWindow: func(window WindowContext) WindowBinding {
			id, _ := window.Context.Value(windowID{}).(string)
			view := &instance{visible: window.Visible}
			instances[id] = view
			return WindowBinding{
				Render: func(*ui.Context) {},
				Slots: func() WorkspaceSlots {
					return WorkspaceSlots{PreferenceKey: id, Navigation: func(*ui.Context) {}}
				},
				Sync:  func() { view.synced++ },
				Close: func() { view.closed++ },
			}
		},
	}
	fixture := New(preview.Load(), page)
	fixture.BindWindow(t.Context())
	if len(instances) != 0 {
		t.Fatal("preview acquired a live window binding")
	}
	open := func(id string) *App {
		app := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, page)
		app.BindWindow(context.WithValue(t.Context(), windowID{}, id))
		return app
	}
	first, second := open("first"), open("second")
	first.Navigate(route)
	first.SyncNativePages()
	first.syncNativeWorkspace()
	if !instances["first"].visible() || instances["second"].visible() || !first.HasNavigation() || first.pluginWorkspace != "first" {
		t.Fatal("the binding did not project only the active window's workspace")
	}
	if instances["first"].synced != 1 || instances["second"].synced != 0 {
		t.Fatal("sync reached a different window")
	}
	first.CloseWindowBindings()
	first.CloseWindowBindings()
	if instances["first"].closed != 1 || instances["first"].visible() || first.HasNavigation() || instances["second"].closed != 0 {
		t.Fatal("closing one binding did not release only that window's callbacks and slots")
	}
	second.CloseWindowBindings()
}
