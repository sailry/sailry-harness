package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func TestResourceWorkspaceDetails(t *testing.T) {
	a, tester := newTester(t)
	a.PanelWidth, a.PanelOpen = 520, false
	details := i18n.In("en", "details")
	pages := []struct {
		page  Page
		key   string
		row   string
		width float32
	}{
		{FilesPage, "page:files", "go.mod", 380},
		{GitPage, "page:git", "README.md", 420},
	}
	row := func(name string) ui.AccessInfo {
		t.Helper()
		var result ui.AccessInfo
		count := 0
		for _, entry := range tester.AccessAll() {
			if entry.Role == ui.RoleTreeItem && entry.Name == name {
				result = entry
				count++
			}
		}
		if count != 1 {
			t.Fatalf("%q appeared in %d resource lists, want one", name, count)
		}
		return result
	}
	for _, page := range pages {
		a.Navigate(page.page)
		tester.Frame()
		state := a.pluginWorkspacePrefs()
		if state.Width != 320 || state.Folded || a.pluginWorkspace != page.key {
			t.Fatalf("page %v initial details: %+v (%q)", page.page, state, a.pluginWorkspace)
		}
		if toggle, ok := headerButton(tester, details); !ok || toggle.Disabled {
			t.Fatalf("page %v has no active Shell details toggle: %+v", page.page, toggle)
		}
		if bounds := row(page.row).Bounds; bounds.X < 1280-state.Width {
			t.Fatalf("page %v list is outside the Shell column: %+v", page.page, bounds)
		}
		clickTree(t, tester, page.row)
		still(tester)
		project := a.project()
		if page.page == FilesPage && project.Files.File() != "sailry-go/go.mod" || page.page == GitPage && project.Git.Change() != page.row {
			t.Fatalf("page %v did not open the selected document", page.page)
		}
		clickButton(t, tester, details)
		settle(tester)
		if !state.Folded {
			t.Fatalf("page %v details did not fold", page.page)
		}
		if _, exists := tester.AccessByRole(ui.RoleTreeItem, page.row); exists {
			t.Fatalf("page %v kept a resource list inside its main content", page.page)
		}
		tester.Key(ui.Cmd|ui.Alt, ui.KeyB)
		settle(tester)
		if state.Folded {
			t.Fatalf("page %v shortcut did not reveal its details", page.page)
		}
		row(page.row)
		edge := float32(1280) - state.Width
		delta := state.Width - page.width
		tester.Press(edge, 200)
		tester.Move(edge+delta, 200)
		tester.Release(edge+delta, 200)
		still(tester)
		if state.Width != page.width {
			t.Fatalf("page %v Shell resize saved %v, want %v", page.page, state.Width, page.width)
		}
		if bounds := row(page.row).Bounds; bounds.X < 1280-page.width {
			t.Fatalf("page %v list did not follow the resized column: %+v", page.page, bounds)
		}
		clickTree(t, tester, page.row)
		tester.Frame()
		a.Do("close")
		still(tester)
		if !state.Folded || a.Page != page.page {
			t.Fatalf("page %v native Close Tab did not fold only its details", page.page)
		}
		a.RevealDetails()
		still(tester)
	}
	for _, page := range pages {
		a.Navigate(page.page)
		still(tester)
		if state := a.pluginWorkspacePrefs(); state.Width != page.width || state.Folded {
			t.Fatalf("page %v lost its independent details choice: %+v", page.page, state)
		}
	}
	a.Navigate(FilesPage)
	still(tester)
	clickButton(t, tester, details)
	settle(tester)
	a.Navigate(GitPage)
	tester.Frame()
	row("README.md")
	a.Navigate(FilesPage)
	tester.Frame()
	if !a.pluginWorkspacePrefs().Folded {
		t.Fatal("switching pages lost the Files fold choice")
	}
	if _, exists := tester.AccessByRole(ui.RoleTreeItem, "go.mod"); exists {
		t.Fatal("the first frame of the folded Files page retained another page's details")
	}
	a.RevealDetails()
	still(tester)
	row("go.mod")
	tester.SetSize(620, 820)
	still(tester)
	if _, exists := tester.AccessByRole(ui.RoleTreeItem, "go.mod"); exists || a.pluginWorkspacePrefs().Folded {
		t.Fatal("a narrow window retained a nested list or changed its saved fold choice")
	}
	tester.SetSize(1280, 820)
	still(tester)
	row("go.mod")
	a.Navigate(ConversationPage)
	still(tester)
	if a.PanelWidth != 520 || a.PanelOpen {
		t.Fatal("resource details changed the conversation panel's width or visibility")
	}
	dir := t.TempDir()
	if err := SavePrefs(dir, a.Prefs()); err != nil {
		t.Fatal(err)
	}
	restored := New(preview.Load(), TestHarnessPages()...)
	restored.SetPrefs(LoadPrefs(dir))
	for _, page := range pages {
		restored.SetPluginWorkspace(page.key, 0, 260, 320)
		if state := restored.pluginWorkspacePrefs(); state.Width != page.width || state.Folded {
			t.Fatalf("page %v preferences did not survive relaunch: %+v", page.page, state)
		}
	}
}
