package shell

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/panel"
)

// The rail's features open pages in the main column, never panel tabs.
func TestRailOpensPages(t *testing.T) {
	a, tt := newTester(t)
	clickButton(t, tt, i18n.In("en", "files"))
	tt.Advance(time.Second)
	tt.Frame()
	if a.Page != FilesPage || len(a.Panel.Tabs) != 0 || a.PanelOpen {
		t.Fatalf("Files opened page %d, tabs %v, panel %v", a.Page, a.Panel.Tabs, a.PanelOpen)
	}
	if !tt.HasText("A native desktop Agent workspace.") || !tt.HasText("example.md") {
		t.Fatalf("no files page: %q", tt.Texts())
	}
	snapshot(t, tt, "page-files")
	clickButton(t, tt, i18n.In("en", "git"))
	tt.Frame()
	if a.Page != GitPage || len(a.Panel.Tabs) != 0 || !codeHas(tt, "New text", "workspace: three flat panels") {
		t.Fatalf("Git opened page %d, tabs %v: %q", a.Page, a.Panel.Tabs, tt.Texts())
	}
	snapshot(t, tt, "page-git")
	for _, f := range []struct {
		label string
		page  Page
	}{{"activity", ActivityPage}, {"market_title", MarketPage}, {"shell_sessions", ConversationPage}} {
		clickButton(t, tt, i18n.In("en", f.label))
		tt.Frame()
		if a.Page != f.page {
			t.Errorf("%s opened page %d", f.label, a.Page)
		}
	}
	// Selecting a conversation over a project page goes back to it.
	a.Navigate(FilesPage)
	tt.Frame()
	click(t, tt, "Port the neutral theme tokens")
	tt.Frame()
	if a.Page != ConversationPage {
		t.Errorf("selecting a conversation kept page %d", a.Page)
	}
}

// The Files and Git page and tab of a project share one state; another
// project keeps its own.
func TestPageAndTabShareProjectState(t *testing.T) {
	a, tt := newTester(t)
	first := a.project()
	if first == nil || a.project() != first {
		t.Fatal("the selected project has no single state")
	}
	a.Navigate(FilesPage)
	tt.Frame()
	clickTree(t, tt, "go.mod")
	tt.Frame()
	if first.Files.File() != "sailry-go/go.mod" {
		t.Fatalf("the page showed %q", first.Files.File())
	}
	// The panel's file tabs are the page's open files, the one the page
	// chose shown beside the tree.
	a.Navigate(ConversationPage)
	a.openPanel(panel.Files)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if row, ok := tt.AccessByRole(ui.RoleTreeItem, "go.mod"); !ok || !row.Selected {
		t.Fatalf("the tree does not select the page's file: %q", tt.Texts())
	}
	if !codeHas(tt, "Code editor", "require github.com/egoist/mygo v0.8.3") {
		t.Fatalf("the Files tab does not show the page's file: %q", tt.Texts())
	}
	if want := []panel.Tab{{Kind: panel.Files, ID: "sailry-go/README.md"}, {Kind: panel.Files, ID: "sailry-go/go.mod"}}; !slices.Equal(a.Panel.Tabs, want) {
		t.Fatalf("the panel's tabs %v, want the page's files", a.Panel.Tabs)
	}
	snapshot(t, tt, "tab-files-shared")
	// And the reverse: a change chosen in the Git tab shows on the page.
	a.openPanel(panel.Changes)
	tt.Frame()
	click(t, tt, "docs/example.md")
	tt.Frame()
	if a.Panel.Active != (panel.Tab{Kind: panel.Changes, ID: "docs/example.md"}) || len(a.Panel.Tabs) != 4 || !codeHas(tt, "New text", "search: command palette") {
		t.Fatalf("the change does not show in the Git tab: %v %q", a.Panel.Tabs, tt.Texts())
	}
	a.Navigate(GitPage)
	tt.Frame()
	if first.Git.Change() != "docs/example.md" || !codeHas(tt, "New text", "search: command palette") {
		t.Fatalf("the page does not show the tab's change %q: %q", first.Git.Change(), tt.Texts())
	}
	// Closing a diff on the page's strip closes its panel tab.
	if len(first.Git.Open()) != 2 {
		t.Fatalf("the page's diffs %v", first.Git.Open())
	}
	first.Git.Close("docs/example.md")
	a.Panel.Sync(first)
	if slices.Contains(a.Panel.Tabs, panel.Tab{Kind: panel.Changes, ID: "docs/example.md"}) {
		t.Fatalf("the closed diff kept its tab: %v", a.Panel.Tabs)
	}
	first.Git.Show("docs/example.md")
	// Another project starts from its own state and leaves this one's.
	other := a.Data.Projects[1]
	a.Sidebar.Selected = other.Conversations[0].ID
	a.Navigate(FilesPage)
	tt.Frame()
	second := a.project()
	if second == first || second.ID != other.ID || second.Files.File() == "sailry-go/go.mod" {
		t.Fatalf("project %q shares the state of %q", other.ID, first.ID)
	}
	if !tt.HasText("A native desktop Agent workspace.") {
		t.Errorf("the other project does not show its own file: %q", tt.Texts())
	}
	if first.Files.File() != "sailry-go/go.mod" || first.Git.Change() != "docs/example.md" {
		t.Errorf("the first project lost its state: %q %q", first.Files.File(), first.Git.Change())
	}
}

// The first frame of a page already lays the side list out at its width
// (220), with no frame at the split's minimum.
func TestSplitFirstFrame(t *testing.T) {
	a, tt := newTester(t)
	a.Navigate(FilesPage)
	tt.Frame()
	row, ok := tt.AccessByRole(ui.RoleTreeItem, "sailry-go")
	if !ok {
		t.Fatalf("no tree: %q", tt.Texts())
	}
	// The row fills the list inside its 8 DIP padding, at the window's
	// right edge.
	if got := 1280 - row.Bounds.X + 8; got < 219 || got > 221 || row.Bounds.W < 203 || row.Bounds.W > 205 {
		t.Errorf("the first frame's side list is %v wide, row %v", got, row.Bounds)
	}
	// Both columns fill the page's height from under the tab strip: the
	// list's first row and the editor's toolbar sit at its top.
	save, _ := tt.AccessByRole(ui.RoleButton, i18n.In("en", "files_save"))
	if row.Bounds.Y > 140 || save.Bounds.Y > 140 {
		t.Errorf("the split's columns do not start at the top: tree row %v, Save %v", row.Bounds, save.Bounds)
	}
}
