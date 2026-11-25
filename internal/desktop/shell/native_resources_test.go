package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRegisteredPanelResourceDrivesLauncherMenuShortcutAndRenderer(t *testing.T) {
	const kind panel.Kind = "test-panel-resource"
	const label, body = "Fixture resource", "Fixture renderer"
	tab := panel.Tab{Kind: kind, ID: "opaque-resource-id"}
	opened, tabActions := 0, 0
	var releases []panel.TabRelease
	const tabAction = "Fixture tab action"
	var assembly Assembly
	assembly.RegisterPanelResource(panel.Resource{
		Kind: kind, Label: label, Icon: icons.Folder, Order: 30,
		Mods: ui.Ctrl, Key: ui.KeyJ,
		Open:   func(state *panel.State) { opened++; state.Show(tab) },
		Render: func(c *ui.Context, _ *panel.State, current panel.Tab) { ui.Text(c, body+" "+current.ID) },
		TabMenu: func(_ *ui.Context, menu *ui.Menu, _ *panel.State, current panel.Tab) {
			if menu.Item(tabAction).Chosen() {
				if current != tab {
					t.Fatalf("menu operated on %v, want %v", current, tab)
				}
				tabActions++
			}
		},
		Release: func(_ *panel.State, current panel.Tab, reason panel.TabRelease) {
			if current != tab {
				t.Fatalf("released %v, want %v", current, tab)
			}
			releases = append(releases, reason)
		},
	})
	newWindow := func(assembly Assembly) *App {
		app := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
		app.BindPanelResources(assembly, PanelResourceHost{})
		return app
	}
	app := newWindow(assembly)
	tester := ui.NewTester(func(c *ui.Context) {
		app.shortcuts(c)
		ui.Column(c).Fill().Children(func() {
			panel.Strip(c, app.Panel, nil, kit.KeyText)
			panel.View(c, app.Panel, nil, kit.KeyText)
		})
	}, 640, 400)
	tester.SetLocale("en")
	tester.Frame()
	if err := tester.Click(label); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if opened != 1 || app.Panel.Active != tab || !tester.HasText(body+" "+tab.ID) {
		t.Fatalf("launcher did not open the registered renderer: %v, %q", app.Panel.Tabs, tester.Texts())
	}
	tester.Key(ui.Ctrl, ui.KeyJ)
	tester.Frame()
	if opened != 2 {
		t.Fatalf("the registered shortcut opened the resource %d times including the launcher", opened)
	}
	if err := tester.Click(i18n.In("en", "details")); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	for _, item := range tester.AccessAll() {
		if item.Name == label && (item.Role == ui.RoleMenuItem || item.Role == ui.RoleMenuItemCheckBox || item.Role == ui.RoleMenuItemRadio) {
			tester.ClickAt(item.Bounds.X+item.Bounds.W/2, item.Bounds.Y+item.Bounds.H/2)
			break
		}
	}
	tester.Frame()
	if opened != 3 {
		t.Fatalf("the + menu did not use the registered opener: %q", tester.Texts())
	}

	saved := app.Prefs()
	if !slices.Contains(saved.PanelTabs, tab) {
		t.Fatalf("the contributed tab was not persisted: %v", saved.PanelTabs)
	}
	if err := tester.RightClick(label); err != nil {
		t.Fatal(err)
	}
	close := i18n.In("en", "close")
	if got := tester.Menu(); !slices.Equal(got, []string{tabAction, close}) {
		t.Fatalf("tab menu = %v, want business action and host Close", got)
	}
	if err := tester.ChooseMenuItem(tabAction); err != nil {
		t.Fatal(err)
	}
	if tabActions != 1 || len(releases) != 0 || app.Panel.Active != tab {
		t.Fatal("the business action closed or released its tab")
	}
	if err := tester.RightClick(label); err != nil {
		t.Fatal(err)
	}
	if err := tester.ChooseMenuItem(close); err != nil {
		t.Fatal(err)
	}
	if len(app.Panel.Tabs) != 0 || !slices.Equal(releases, []panel.TabRelease{panel.TabRemoved}) {
		t.Fatalf("standard Close left tabs %v and releases %v", app.Panel.Tabs, releases)
	}

	without := newWindow(Assembly{})
	without.SetPrefs(saved)
	without.Panel.Open(kind)
	without.Panel.New(kind)
	if without.Panel.Offers(kind) || len(without.Panel.Tabs) != 0 || opened != 3 {
		t.Fatal("a window without the resource exposed, restored, or opened its tab")
	}
	without.Panel.Show(panel.Tab{Kind: panel.Documents})
	empty := ui.NewTester(func(c *ui.Context) { panel.Strip(c, without.Panel, nil, kit.KeyText) }, 640, 400)
	empty.SetLocale("en")
	empty.Frame()
	if _, found := empty.AccessByRole(ui.RoleMenuButton, i18n.In("en", "details")); found {
		t.Fatal("the empty assembly showed an add button without any entries")
	}
	if err := empty.RightClick(i18n.In("en", "market_category_documents")); err != nil {
		t.Fatal(err)
	}
	if got := empty.Menu(); !slices.Equal(got, []string{close}) {
		t.Fatalf("a built-in tab inherited a business menu: %v", got)
	}
}

func TestPanelShortcutBindingDrivesWindowAndKeyCaps(t *testing.T) {
	const first, second, mouseOnly = panel.Kind("first"), panel.Changes, panel.Kind("mouse-only")
	manifestKey := "ctrl-j"
	manifest := protocol.PluginDesktopRenderer{Resource: protocol.PluginDesktopResourceKindGit, Shortcut: &manifestKey}
	opened := map[panel.Kind]int{}
	register := func(a *Assembly, kind panel.Kind, order int, key ui.Key, keystroke string) {
		a.RegisterPanelResource(panel.Resource{
			Kind: kind, Label: string(kind), Icon: icons.Folder, Order: order,
			Mods: ui.Ctrl, Key: key, Keystroke: keystroke,
			Open:   func(s *panel.State) { opened[kind]++; s.Show(panel.Tab{Kind: kind}) },
			Render: func(c *ui.Context, _ *panel.State, _ panel.Tab) { ui.Text(c, "resource body") },
		})
	}
	var assembly Assembly
	register(&assembly, first, 10, ui.KeyJ, "")
	register(&assembly, panel.Kind(manifest.Resource), 20, 0, *manifest.Shortcut)
	register(&assembly, mouseOnly, 30, 0, "")
	app := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
	keys := &app.Settings.Keys
	tester := ui.NewTester(func(c *ui.Context) {
		app.shortcuts(c)
		ui.Column(c).Fill().Children(func() {
			panel.Strip(c, app.Panel, nil, kit.KeyText)
			panel.View(c, app.Panel, nil, kit.KeyText)
		})
	}, 640, 400)
	tester.SetLocale("en")
	oldCap, newCap := kit.KeyText(ui.Ctrl, ui.KeyJ), kit.KeyText(ui.Ctrl, ui.KeyU)
	tester.Frame()
	count := 0
	for _, text := range tester.Texts() {
		if text == oldCap {
			count++
		}
	}
	if count != 1 {
		t.Fatalf("conflicting declarations rendered %d copies of %q", count, oldCap)
	}
	tester.Key(ui.Ctrl, ui.KeyJ)
	tester.Frame()
	if opened[first] != 1 || opened[second] != 0 {
		t.Fatalf("default conflict dispatched %v", opened)
	}
	app.Panel.Clear()
	keys.Record(settings.PanelShortcutID(string(first)))
	if err := keys.Capture(settings.Chord{Mods: ui.Ctrl, Key: ui.KeyU}); err != "" {
		t.Fatalf("rebinding failed: %s", err)
	}
	tester.Frame()
	if !tester.HasText(oldCap) || !tester.HasText(newCap) {
		t.Fatalf("launcher does not show the effective bindings: %v", tester.Texts())
	}
	tester.Key(ui.Ctrl, ui.KeyU)
	tester.Frame()
	if opened[first] != 2 || opened[second] != 0 {
		t.Fatalf("rebound shortcut dispatched %v", opened)
	}
	if err := tester.Click(i18n.In("en", "details")); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText(oldCap) || !tester.HasText(newCap) || !tester.HasText(string(mouseOnly)) {
		t.Fatalf("add menu does not use the same bindings or lost the mouse-only resource: %v", tester.Texts())
	}
	tester.Key(0, ui.KeyEscape)
	keys.Record(settings.PanelShortcutID(string(first)))
	keys.Clear()
	app.Panel.Clear()
	tester.Frame()
	if tester.HasText(newCap) || !tester.HasText(string(first)) {
		t.Fatal("unbinding left a key cap or removed the action")
	}
	tester.Key(ui.Ctrl, ui.KeyU)
	tester.Frame()
	if opened[first] != 2 {
		t.Fatal("an unbound key dispatched")
	}
	if err := tester.Click(string(first)); err != nil {
		t.Fatal(err)
	}
	if opened[first] != 3 {
		t.Fatal("an unbound operation stopped working from the launcher")
	}

	id := settings.PanelShortcutID(string(first))
	app.Panel.SetResources(nil)
	prefs := keys.Overrides()
	if val, ok := prefs[id]; !ok || val != "" || app.Panel.Active != (panel.Tab{}) {
		t.Fatalf("retiring the resource changed its saved override or left its tab: %v, %v", prefs, app.Panel.Active)
	}
	without := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	without.Settings.Keys.SetOverrides(prefs)
	if val, ok := without.Settings.Keys.Overrides()[id]; !ok || val != "" {
		t.Fatal("restarting without the resource forgot its unbound preference")
	}
}
