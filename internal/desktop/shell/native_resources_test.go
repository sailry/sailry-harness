package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestRegisteredPanelResourceDrivesLauncherMenuShortcutAndRenderer(t *testing.T) {
	const kind panel.Kind = "test-panel-resource"
	const label, body = "Fixture resource", "Fixture renderer"
	tab := panel.Tab{Kind: kind, ID: "opaque-resource-id"}
	opened := 0
	var assembly Assembly
	assembly.RegisterPanelResource(panel.Resource{
		Kind: kind, Label: label, Icon: icons.Folder, Order: 30,
		Mods: ui.Ctrl, Key: ui.KeyJ,
		Open:   func(state *panel.State) { opened++; state.Show(tab) },
		Render: func(c *ui.Context, _ *panel.State, current panel.Tab) { ui.Text(c, body+" "+current.ID) },
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
	without := newWindow(Assembly{})
	without.SetPrefs(saved)
	without.Panel.Open(kind)
	if without.Panel.Offers(kind) || len(without.Panel.Tabs) != 0 || opened != 3 {
		t.Fatal("a window without the resource exposed, restored, or opened its tab")
	}
}
