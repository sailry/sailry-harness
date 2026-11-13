package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func TestPluginWorkspace(t *testing.T) {
	t.Run("navigation uses the Shell column and toggle", func(t *testing.T) {
		a, tester := newTester(t)
		a.Hooks.PluginPage = func(view *ui.Context, _ string) { ui.Text(view, "Workspace content") }
		a.Navigate(PluginPage)
		still(tester)
		if a.HasNavigation() || sessionsShown(tester) {
			t.Fatal("a package without navigation kept the session sidebar")
		}
		calls := 0
		a.PluginNavigation = func(view *ui.Context) {
			calls++
			ui.Text(view.Key("resource"), "Resource tree")
		}
		a.SetPluginWorkspace("fixture", 280, 260, 400)
		tester.Frame()
		if calls != 1 || !a.HasNavigation() || !a.NavShown() || sessionsShown(tester) {
			t.Fatalf("resource navigation calls=%d available=%v shown=%v", calls, a.HasNavigation(), a.NavShown())
		}
		resource, shown := tester.Find("Resource tree")
		content, present := tester.Find("Workspace content")
		if !shown || !present || resource.X < RailWidth || resource.X >= RailWidth+280 || content.X < RailWidth+280 {
			t.Fatalf("resource=%v content=%v, want the application sidebar", resource, content)
		}
		tester.Key(ui.Cmd, ui.KeyB)
		settle(tester)
		if a.NavOpen || tester.HasText("Resource tree") {
			t.Fatal("the navigation shortcut did not fold the resource tree")
		}
		clickButton(t, tester, i18n.In("en", "sidebar_toggle"))
		settle(tester)
		if !a.NavShown() || !tester.HasText("Resource tree") {
			t.Fatal("the header toggle did not restore the resource tree")
		}
		edge := float32(RailWidth + 280 + 1)
		tester.Press(edge, 200)
		tester.Move(edge+40, 200)
		tester.Release(edge+40, 200)
		still(tester)
		if a.NavWidth != 320 {
			t.Fatalf("dragging the visible navigation edge saved %v, want 320", a.NavWidth)
		}
		a.NavWidth = 310
		a.Navigate(ConversationPage)
		still(tester)
		if !sessionsShown(tester) || tester.HasText("Resource tree") || a.NavWidth != 310 {
			t.Fatal("returning to the conversation did not restore its sidebar")
		}
		a.Navigate(PluginPage)
		tester.SetSize(700, 820)
		still(tester)
		if !tester.HasText("Resource tree") || !a.NavShown() || a.NavWidth != 310 {
			t.Fatal("narrowing the window changed the saved navigation choice")
		}
		a.PluginNavigation = nil
		still(tester)
		if a.HasNavigation() || tester.HasText("Resource tree") || !a.NavOpen {
			t.Fatal("removing resource navigation kept a column or changed its saved choice")
		}
	})
	t.Run("details keep each workspace's width and fold choice", func(t *testing.T) {
		a, tester := newTester(t)
		a.Hooks.PluginPage = func(view *ui.Context, _ string) { ui.Text(view, "Workspace content") }
		a.PluginDetails = func(view *ui.Context) { ui.Text(view, "Workspace details") }
		a.PanelWidth, a.PanelOpen = 520, false
		a.Navigate(PluginPage)
		a.SetPluginWorkspace("first", 0, 300, 900)
		if got := a.pluginWorkspacePrefs().Width; got != 480 {
			t.Fatalf("initial width %v, want clamped default 480", got)
		}
		a.pluginWorkspacePrefs().Width = 350
		a.togglePanel()
		a.SetPluginWorkspace("second", 900, -20, 100)
		if a.pluginWorkspacePrefs().Width != 260 || a.pluginWorkspacePrefs().Folded || a.clampNavigation(200) != 320 {
			t.Fatal("the second workspace inherited the first workspace's choices")
		}
		a.SetPluginWorkspace("first", 280, 300, 400)
		if a.pluginWorkspacePrefs().Width != 350 || !a.pluginWorkspacePrefs().Folded {
			t.Fatal("reprojection reset the user's width or fold choice")
		}
		// Packages reveal after selection, before the next render supplies
		// its new details child. That interval still belongs to the package.
		a.PluginDetails = nil
		a.RevealDetails()
		if a.pluginWorkspacePrefs().Folded || a.PanelOpen {
			t.Fatal("revealing pending plugin details changed the conversation's choice")
		}
		a.PluginDetails = func(view *ui.Context) { ui.Text(view, "Workspace details") }
		still(tester)
		if !tester.HasText("Workspace details") || a.PanelOpen || a.PanelWidth != 520 {
			t.Fatal("plugin details changed the conversation panel")
		}
		a.PluginDetailsLabel = "Assistant"
		still(tester)
		if toggle, ok := headerButton(tester, "Assistant"); !ok || toggle.Disabled {
			t.Fatal("the workspace's details label did not name the toggle")
		}
		if got := a.clampPluginDetails(100, 1280, 280); got != 300 {
			t.Fatalf("details minimum %v, want 300", got)
		}
		if got := a.clampPluginDetails(900, 1280, 280); got != 480 {
			t.Fatalf("details maximum %v, want 480", got)
		}
		tester.SetSize(650, 820)
		still(tester)
		if tester.HasText("Workspace details") || a.pluginWorkspacePrefs().Folded {
			t.Fatal("a narrow window folded the workspace's saved choice")
		}
		dir := t.TempDir()
		if err := SavePrefs(dir, a.Prefs()); err != nil {
			t.Fatal(err)
		}
		b := New(preview.Load())
		b.SetPrefs(LoadPrefs(dir))
		b.SetPluginWorkspace("first", 280, 300, 400)
		if b.pluginWorkspacePrefs().Width != 350 || b.pluginWorkspacePrefs().Folded || b.PanelWidth != 520 || b.PanelOpen {
			t.Fatalf("workspace preferences were not restored independently: %+v", b.Prefs())
		}
		b.SetPluginWorkspace("second", 200, 480, 400)
		if b.pluginWorkspacePrefs().Width != 480 {
			t.Fatal("updated minimum did not clamp the saved workspace width")
		}
	})
}
