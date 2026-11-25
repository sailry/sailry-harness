package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
)

func TestWorkspaceTerminalStandalone(t *testing.T) {
	a, tt := newTester(t)
	selected, stopped := a.Sidebar.Selected, 0
	a.Panel.TerminalActions = &panel.TerminalActions{
		Pane:  func(c *ui.Context, id string) { ui.Text(c, "main terminal "+id) },
		Close: func(string) { stopped++ },
	}
	a.OpenWorkspaceTerminal("project-terminal", "Project shell")
	tt.Frame()
	tt.Frame()
	if !tt.HasText("main terminal project-terminal") || !tt.HasText("Project shell") || a.panes.Displayed() != nil {
		t.Fatalf("standalone terminal: %q, groups %v", tt.Texts(), a.panes.Groups)
	}
	if a.FocusedConversation() != "" || a.findTarget() != "" || len(a.AllPanelTabs()) != 0 {
		t.Fatalf("main terminal kept conversation scope or panel tabs: active %v, tabs %v", a.panes.Active, a.AllPanelTabs())
	}
	for _, id := range []string{"conversation.stop", "composer.focus", "composer.models"} {
		a.Do(id)
		tt.Frame()
		if a.keyed != "" {
			t.Fatalf("terminal queued a hidden conversation command %q", a.keyed)
		}
	}
	a.ShowSelection("Selected text")
	if a.selected.chat != "" {
		t.Fatalf("selection targets hidden conversation %q", a.selected.chat)
	}
	a.selected.open = false
	r, ok := tt.Find("main terminal project-terminal")
	if !ok {
		t.Fatal("no terminal screen")
	}
	tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
	tt.Frame()
	a.Do(CloseFocused.ID)
	tt.Frame()
	if a.HasWorkspaceTerminal("project-terminal") || stopped != 0 || a.Sidebar.Selected != selected {
		t.Fatalf("close surface: terminals %v, stopped %d, selected %q", a.WorkspaceTerminalIDs(), stopped, a.Sidebar.Selected)
	}
}

func TestWorkspaceTerminalSameConversationSelection(t *testing.T) {
	a, tt := newTester(t)
	selected := a.Sidebar.Selected
	a.OpenWorkspaceTerminal("terminal", "Project shell")
	tt.Frame()
	click(t, tt, "Virtualized transcript stress test")
	if a.panes.Active != session(selected) || a.FocusedConversation() != selected || !a.HasWorkspaceTerminal("terminal") {
		t.Fatalf("same-row selection: active %v, terminals %v", a.panes.Active, a.WorkspaceTerminalIDs())
	}
}

// The sidebar selects a main terminal's row only while the conversation page
// shows it; another page with the sidebar leaves the selection to it.
func TestWorkspaceTerminalSidebarSelection(t *testing.T) {
	a, tt := newTester(t)
	a.OpenWorkspaceTerminal("terminal", "Project shell")
	tt.Frame()
	if a.Sidebar.Terminal != "terminal" {
		t.Fatalf("the shown terminal's row is not selected: %q", a.Sidebar.Terminal)
	}
	a.OpenProject(a.Data.Projects[0], ProjectActions{})
	tt.Frame()
	if a.Page != ProjectPage || a.Sidebar.Terminal != "" {
		t.Fatalf("the terminal's row stayed selected on page %v: %q", a.Page, a.Sidebar.Terminal)
	}
}

func TestWorkspaceTerminalStandaloneSplitKeys(t *testing.T) {
	a, tt := newTester(t)
	a.Panel.TerminalActions = &panel.TerminalActions{
		Pane: func(c *ui.Context, id string) { ui.Text(c, "main terminal "+id) },
	}
	a.OpenWorkspaceTerminal("original", "Project shell")
	tt.Frame()
	r, ok := tt.Find("main terminal original")
	if !ok {
		t.Fatal("no terminal screen")
	}
	tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyD)
	tt.Frame()
	g := a.panes.Displayed()
	if g == nil || g.Axis != panes.Row || len(g.Targets()) != 2 || len(a.AllPanelTabs()) != 0 {
		t.Fatalf("standalone terminal split: groups %v, panel %v", a.panes.Groups, a.AllPanelTabs())
	}
	for _, target := range g.Targets() {
		if target.Kind != panes.Terminal || !a.HasWorkspaceTerminal(target.ID) {
			t.Fatalf("split target has no independent surface: %v", target)
		}
	}
	left := g.Targets()[0]
	a.DropWorkspaceTerminal(a.panes.Active.ID)
	tt.Frame()
	if a.panes.Displayed() != nil || a.panes.Active != left || !tt.HasText("main terminal "+left.ID) {
		t.Fatalf("last terminal cannot stand alone: active %v, groups %v", a.panes.Active, a.panes.Groups)
	}
}

func TestWorkspaceTerminalsPersistSeparately(t *testing.T) {
	a, _ := newTester(t)
	a.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: "sidebar-terminal"})
	a.OpenWorkspaceTerminal("main-terminal", "Project shell")
	prefs := a.Prefs()
	if len(prefs.WorkspaceTerminals) != 1 || prefs.WorkspaceTerminals[0].ID != "main-terminal" ||
		!slices.Contains(prefs.PanelTabs, panel.Tab{Kind: panel.Terminal, ID: "sidebar-terminal"}) {
		t.Fatalf("terminal ownership not saved separately: %+v", prefs)
	}
	b, _ := newTester(t)
	b.SetPrefs(prefs)
	if !b.HasWorkspaceTerminal("main-terminal") || b.HasWorkspaceTerminal("sidebar-terminal") ||
		b.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: "main-terminal"}) || b.workspaceTerminal("main-terminal").Title != "Project shell" {
		t.Fatalf("restored ownership: main %v, tabs %v", b.WorkspaceTerminalIDs(), b.AllPanelTabs())
	}
}
