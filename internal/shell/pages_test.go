package shell

import (
	"testing"
	"time"

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
	if !tt.HasText("A native desktop Agent workspace.") || !tt.HasText(i18n.In("en", "file_tree")) {
		t.Fatalf("no files page: %q", tt.Texts())
	}
	snapshot(t, tt, "page-files")
	clickButton(t, tt, i18n.In("en", "git"))
	tt.Frame()
	if a.Page != GitPage || len(a.Panel.Tabs) != 0 || !tt.HasText("workspace: three flat panels") {
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
	click(t, tt, "go.mod")
	tt.Frame()
	if first.Files.File() != "sailry-go/go.mod" {
		t.Fatalf("the page showed %q", first.Files.File())
	}
	// The panel's Files tab shows the file the page chose.
	a.Navigate(ConversationPage)
	a.openPanel(panel.Files)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if !tt.HasText("require github.com/egoist/mygo v0.8.3") {
		t.Fatalf("the tab does not show the page's file: %q", tt.Texts())
	}
	snapshot(t, tt, "tab-files-shared")
	// And the reverse: a change chosen in the Git tab shows on the page.
	a.openPanel(panel.Changes)
	tt.Frame()
	click(t, tt, "docs/example.md")
	tt.Frame()
	a.Navigate(GitPage)
	tt.Frame()
	if first.Git.Change() != "docs/example.md" || !tt.HasText("search: command palette") {
		t.Fatalf("the page does not show the tab's change %q: %q", first.Git.Change(), tt.Texts())
	}
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
